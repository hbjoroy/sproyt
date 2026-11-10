use super::*;
use futures_util::poll;
use std::time::{Duration, Instant};

fn count(metrics: &CapacityMetrics, phase: &str, result: &str) -> u64 {
    let text = metrics.render();
    text.lines()
        .filter(|l| l.starts_with("sproyt_chat_phase_duration_seconds_count{"))
        .filter(|l| l.contains(&format!("phase=\"{phase}\",result=\"{result}\"")))
        .map(|l| l.rsplit(' ').next().unwrap().parse::<u64>().unwrap())
        .sum()
}
fn gauge(metrics: &CapacityMetrics, phase: &str) -> u64 {
    let text = metrics.render();
    let prefix = format!("sproyt_chat_phase_in_flight{{phase=\"{phase}\"}} ");
    text.lines()
        .find_map(|l| l.strip_prefix(&prefix))
        .unwrap()
        .parse()
        .unwrap()
}

#[tokio::test]
async fn bounded_mailbox_preserves_fifo_and_cancellation_before_admission() {
    let metrics = Arc::new(CapacityMetrics::default());
    let (mailbox, mut receiver) = Mailbox::with_metrics(1, metrics.clone());
    mailbox.send(Command::ExpirePresence).await.unwrap();
    let mut cancelled = Box::pin(mailbox.send(Command::ExternalMessage {
        message_id: crate::domain::MessageId::generate(),
    }));
    assert!(poll!(&mut cancelled).is_pending());
    let mut waiting = Box::pin(mailbox.send(Command::ChannelCreated {
        source_channel_id: crate::domain::ChannelId::generate(),
        channel_id: crate::domain::ChannelId::generate(),
        created_by: crate::domain::UserId::named("capacity-fixture"),
    }));
    assert!(poll!(&mut waiting).is_pending());
    assert_eq!(gauge(&metrics, "admission"), 2);
    assert_eq!(gauge(&metrics, "queued"), 1);
    drop(cancelled);
    assert_eq!(gauge(&metrics, "admission"), 1);
    let (command, execution) = receiver.recv().await.unwrap().begin();
    assert!(matches!(command, Command::ExpirePresence));
    assert_eq!(gauge(&metrics, "execution"), 1);
    waiting.await.unwrap();
    execution.finish();
    let (command, execution) = receiver.recv().await.unwrap().begin();
    assert!(matches!(command, Command::ChannelCreated { .. }));
    execution.finish();
    assert_eq!(count(&metrics, "admission", "admitted"), 2);
    assert_eq!(count(&metrics, "admission", "cancelled"), 1);
    assert_eq!(count(&metrics, "queued", "dequeued"), 2);
    assert_eq!(count(&metrics, "execution", "completed"), 2);
    for phase in ["admission", "queued", "execution"] {
        assert_eq!(gauge(&metrics, phase), 0);
    }
    assert!(!metrics.render().contains("capacity-fixture"));
}

#[tokio::test]
async fn receiver_drop_records_discarded_queue_and_closed_admission() {
    let metrics = Arc::new(CapacityMetrics::default());
    let (mailbox, receiver) = Mailbox::with_metrics(1, metrics.clone());
    mailbox.send(Command::ExpirePresence).await.unwrap();
    let mut waiting = Box::pin(mailbox.send(Command::ExpirePresence));
    assert!(poll!(&mut waiting).is_pending());
    drop(receiver);
    assert!(waiting.await.is_err());
    assert_eq!(count(&metrics, "queued", "discarded"), 1);
    assert_eq!(count(&metrics, "admission", "closed"), 1);
    for phase in ["admission", "queued", "execution"] {
        assert_eq!(gauge(&metrics, phase), 0);
    }
}

#[tokio::test]
async fn execution_abort_and_dropped_reply_do_not_undo_admission() {
    let metrics = Arc::new(CapacityMetrics::default());
    let (mailbox, mut receiver) = Mailbox::with_metrics(1, metrics.clone());
    let (reply, response) = tokio::sync::oneshot::channel();
    mailbox
        .send(Command::Subscribe {
            channel_id: crate::domain::ChannelId::generate(),
            participant_id: crate::domain::UserId::named("dropped-reply"),
            reply,
        })
        .await
        .unwrap();
    drop(response);
    let envelope = receiver.recv().await.unwrap();
    let (started, ready) = tokio::sync::oneshot::channel();
    let actor = tokio::spawn(async move {
        let (command, execution) = envelope.begin();
        assert!(matches!(command, Command::Subscribe { .. }));
        started.send(()).unwrap();
        std::future::pending::<()>().await;
        execution.finish();
    });
    ready.await.unwrap();
    assert_eq!(gauge(&metrics, "execution"), 1);
    actor.abort();
    assert!(actor.await.unwrap_err().is_cancelled());
    assert_eq!(count(&metrics, "admission", "admitted"), 1);
    assert_eq!(count(&metrics, "execution", "cancelled"), 1);
    assert_eq!(gauge(&metrics, "execution"), 0);
}

#[tokio::test]
async fn phase_timings_separate_waiting_for_space_queue_and_execution() {
    let metrics = Arc::new(CapacityMetrics::default());
    let (mailbox, mut receiver) = Mailbox::with_metrics(1, metrics.clone());
    mailbox.send(Command::ExpirePresence).await.unwrap();
    let mut waiting = Box::pin(mailbox.send(Command::ExpirePresence));
    assert!(poll!(&mut waiting).is_pending());
    tokio::time::sleep(Duration::from_millis(10)).await;
    let (_, execution) = receiver.recv().await.unwrap().begin();
    waiting.await.unwrap();
    tokio::time::sleep(Duration::from_millis(10)).await;
    execution.finish();
    receiver.recv().await.unwrap().begin().1.finish();
    let text = metrics.render();
    for phase in ["admission", "queued", "execution"] {
        let line = text
            .lines()
            .find(|l| {
                l.starts_with("sproyt_chat_phase_duration_seconds_sum{")
                    && l.contains(&format!("phase=\"{phase}\""))
            })
            .unwrap();
        assert!(line.rsplit(' ').next().unwrap().parse::<f64>().unwrap() >= 0.01);
    }
}

#[tokio::test]
#[ignore = "diagnostic timing sample; run explicitly with --nocapture"]
async fn mailbox_instrumentation_overhead_sample() {
    const N: u32 = 10_000;
    // Same task and capacity, no producer parking; measures instrumentation
    // overhead only, not chat/database throughput or target-environment SLOs.
    for repetition in 1..=3 {
        let (sender, mut receiver) = mpsc::channel(1);
        let started = Instant::now();
        for _ in 0..N {
            sender.send(Command::ExpirePresence).await.unwrap();
            receiver.recv().await.unwrap();
        }
        let baseline = started.elapsed();
        let metrics = Arc::new(CapacityMetrics::default());
        let (sender, mut receiver) = Mailbox::with_metrics(1, metrics.clone());
        let started = Instant::now();
        for _ in 0..N {
            sender.send(Command::ExpirePresence).await.unwrap();
            receiver.recv().await.unwrap().begin().1.finish();
        }
        let measured = started.elapsed();
        eprintln!(
            "mailbox overhead repetition={repetition} commands={N} baseline_us={} measured_us={} added_ns_per_command={}",
            baseline.as_micros(),
            measured.as_micros(),
            measured.saturating_sub(baseline).as_nanos() / u128::from(N)
        );
        assert_eq!(count(&metrics, "execution", "completed"), u64::from(N));
    }
}
