//! Isolated, sequential preview worker. Decoding runs in a killable subprocess.
use crate::media_store::{
    CHUNK_BYTES, MediaStore, UploadLimits,
    preview::{PREVIEW_MAX_BYTES, PreviewJob},
};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    sync::watch,
};
type Error = Box<dyn std::error::Error + Send + Sync>;

pub(crate) async fn run() -> Result<(), Error> {
    let database = crate::config::AppConfig::database_from_env()?;
    let pool = crate::db::connect_postgres_pool(&database, 2).await?;
    let store = MediaStore::connect(
        &database,
        pool.as_ref(),
        UploadLimits {
            max_file_bytes: 1024 * 1024 * 1024,
            owner_bytes: 1024 * 1024 * 1024,
            total_bytes: 1024 * 1024 * 1024,
        },
    )
    .await?;
    let scratch = PathBuf::from(
        std::env::var("SPROYT_MEDIA_SCRATCH_DIR").unwrap_or_else(|_| "/tmp/media-worker".into()),
    );
    tokio::fs::create_dir_all(&scratch).await?;
    // Private single-worker/pod directory: recover files left by SIGKILL/OOM.
    let mut entries = tokio::fs::read_dir(&scratch).await?;
    while let Some(entry) = entries.next_entry().await? {
        if entry.file_name().to_string_lossy().starts_with("preview-")
            && entry.file_type().await?.is_dir()
        {
            tokio::fs::remove_dir_all(entry.path()).await?;
        }
    }
    let (sender, mut shutdown) = watch::channel(false);
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            if let Ok(mut terminate) =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            {
                tokio::select! { _=tokio::signal::ctrl_c()=>{}, _=terminate.recv()=>{} }
            }
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
        }
        let _ = sender.send(true);
    });
    while !*shutdown.borrow() {
        let job = tokio::select! {
            _=shutdown.changed()=>break,
            r=store.claim_preview()=>match r {
                Ok(job)=>job,
                Err(error)=>{tracing::warn!(%error,"preview claim failed");None},
            }
        };
        let Some(job) = job else {
            tokio::select! {_=shutdown.changed()=>break, _=tokio::time::sleep(Duration::from_secs(3))=>{}}
            continue;
        };
        let directory = scratch.join(format!("preview-{}", job.token));
        tokio::fs::create_dir(&directory).await?;
        let result = process(&store, &job, &directory, &mut shutdown).await;
        // process() explicitly kills AND reaps before returning on cancellation.
        tokio::fs::remove_dir_all(&directory).await?;
        match result {
            Ok(()) => tracing::info!(media_id=%job.media_id,"preview ready"),
            Err(_) => {
                store.fail_preview(&job).await?;
                tracing::warn!(media_id=%job.media_id,"preview failed; original retained");
            }
        }
    }
    Ok(())
}

async fn write_original(store: &MediaStore, job: &PreviewJob, input: &Path) -> Result<(), Error> {
    let mut file = tokio::fs::File::create(input).await?;
    let mut remaining = job.size_bytes;
    let mut index = 0;
    let mut hash = Sha256::new();
    while remaining > 0 {
        let bytes = store.preview_chunk(job, index).await?;
        let expected = remaining.min(CHUNK_BYTES as u64) as usize;
        if bytes.len() != expected {
            return Err("incomplete original".into());
        }
        hash.update(&bytes);
        file.write_all(&bytes).await?;
        remaining -= bytes.len() as u64;
        index += 1;
    }
    file.flush().await?;
    if format!("{:x}", hash.finalize()) != job.sha256 {
        return Err("original hash mismatch".into());
    }
    Ok(())
}

async fn process(
    store: &MediaStore,
    job: &PreviewJob,
    directory: &Path,
    shutdown: &mut watch::Receiver<bool>,
) -> Result<(), Error> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
    let input = directory.join("original");
    let output = directory.join("preview.jpg");
    if *shutdown.borrow() {
        return Err("shutdown".into());
    }
    tokio::select! {
        _=shutdown.changed()=>return Err("shutdown".into()),
        _=tokio::time::sleep_until(deadline)=>return Err("preview timeout".into()),
        r=write_original(store,job,&input)=>r?,
    }
    // Fixed paths and arguments, no shell or user filenames. Shrink-on-load,
    // default autorotation, bounded sRGB JPEG; never re-encode the original.
    let mut child = Command::new("/usr/bin/vipsthumbnail")
        .arg(&input)
        .args(["--size", "720x720", "--output-profile", "srgb", "--output"])
        .arg(format!(
            "{}[Q=85,keep=none,optimize_coding]",
            output.display()
        ))
        .env("VIPS_CONCURRENCY", "1")
        .env("VIPS_DISC_THRESHOLD", "16m")
        .env("TMPDIR", directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    wait_codec(&mut child, deadline, shutdown).await?;
    let publish = async {
        let file = tokio::fs::File::open(&output).await?;
        let mut bytes = Vec::new();
        file.take((PREVIEW_MAX_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .await?;
        if bytes.len() > PREVIEW_MAX_BYTES {
            return Err::<(), Error>("preview too large".into());
        }
        let reader = image::ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format()?;
        if reader.format() != Some(image::ImageFormat::Jpeg) {
            return Err("invalid preview type".into());
        }
        let (width, height) = reader.into_dimensions()?;
        store.publish_preview(job, bytes, width, height).await?;
        Ok(())
    };
    tokio::select! {
        _=shutdown.changed()=>Err("shutdown".into()),
        _=tokio::time::sleep_until(deadline)=>Err("preview timeout".into()),
        r=publish=>r,
    }
}

async fn wait_codec(
    child: &mut tokio::process::Child,
    deadline: tokio::time::Instant,
    shutdown: &mut watch::Receiver<bool>,
) -> Result<(), Error> {
    let status = tokio::select! {
        _=shutdown.changed()=>None,
        _=tokio::time::sleep_until(deadline)=>None,
        r=child.wait()=>Some(r),
    };
    let Some(status) = status else {
        // Tokio kill() waits for termination; wait() also makes reaping explicit.
        child.kill().await?;
        child.wait().await?;
        return Err("preview cancelled".into());
    };
    if !status?.success() {
        return Err("codec failed".into());
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[tokio::test]
    async fn timeout_and_shutdown_reap_codec_before_returning() {
        for shutdown_now in [false, true] {
            let mut child = Command::new("/bin/sleep")
                .arg("60")
                .kill_on_drop(true)
                .spawn()
                .unwrap();
            let (sender, mut receiver) = watch::channel(false);
            let deadline = tokio::time::Instant::now()
                + Duration::from_millis(if shutdown_now { 10000 } else { 50 });
            if shutdown_now {
                sender.send(true).unwrap();
            }
            assert!(
                wait_codec(&mut child, deadline, &mut receiver)
                    .await
                    .is_err()
            );
            assert!(
                child.id().is_none(),
                "codec must be reaped before scratch cleanup"
            );
            assert!(child.try_wait().unwrap().is_some());
        }
    }
}
