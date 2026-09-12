//! Durable, owner-only image requests. The ComfyUI address and graph are server
//! configuration: clients can supply text, never graph nodes or output URLs.
use std::{sync::Arc, time::Duration};

use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{PgPool, SqlitePool};
use tokio::sync::watch;
use uuid::Uuid;

use crate::{
    config::{DatabaseConfig, DatabaseKind},
    domain::{MediaObject, UserId},
};

const MAX_IMAGE: usize = 8 * 1024 * 1024;
const RETENTION: i64 = 7 * 24 * 3600;

#[derive(Clone)]
enum Store {
    Postgres(PgPool),
    Sqlite(SqlitePool),
}

#[derive(Clone)]
pub(crate) struct ImageGeneration {
    store: Store,
    gateway: Arc<Gateway>,
}

struct Gateway {
    base: String,
    http: reqwest::Client,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Job {
    pub id: String,
    pub owner_id: UserId,
    pub channel_id: crate::domain::ChannelId,
    pub state: String,
    pub prompt: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub revision: i64,
    pub prompt_id: Option<String>,
    pub image: Option<String>,
    pub media: Option<MediaObject>,
    pub error: Option<String>,
}

impl Job {
    pub fn view(&self) -> Value {
        json!({"id":self.id,"channel_id":self.channel_id,"state":self.state,
            "prompt":self.prompt,"created_at":self.created_at,"error":self.error,
            "media":self.media})
    }
    pub fn transition(&mut self, state: &str) {
        self.state = state.into();
        self.updated_at = now();
    }
}

type Error = Box<dyn std::error::Error + Send + Sync>;
fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

impl ImageGeneration {
    pub async fn from_env(config: &DatabaseConfig) -> Result<Option<Self>, Error> {
        let Ok(base) = std::env::var("SPROYT_COMFYUI_URL") else {
            return Ok(None);
        };
        let url = reqwest::Url::parse(&base)?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("invalid SPROYT_COMFYUI_URL".into());
        }
        let store = match config.kind() {
            DatabaseKind::Postgres => Store::Postgres(PgPool::connect(config.url()).await?),
            DatabaseKind::Sqlite => Store::Sqlite(SqlitePool::connect(config.url()).await?),
        };
        Ok(Some(Self {
            store,
            gateway: Arc::new(Gateway {
                base: base.trim_end_matches('/').into(),
                http: reqwest::Client::builder()
                    .timeout(Duration::from_secs(20))
                    .redirect(reqwest::redirect::Policy::none())
                    .build()?,
            }),
        }))
    }

    pub async fn get(&self, id: &str) -> Result<Option<Job>, Error> {
        use sqlx::Row;
        let sql = "SELECT data FROM image_generation_jobs WHERE id=$1";
        let data: Option<String> = match &self.store {
            Store::Postgres(pool) => sqlx::query(sql)
                .bind(id)
                .fetch_optional(pool)
                .await?
                .map(|r| r.get("data")),
            Store::Sqlite(pool) => sqlx::query(sql)
                .bind(id)
                .fetch_optional(pool)
                .await?
                .map(|r| r.get("data")),
        };
        data.map(|d| serde_json::from_str(&d).map_err(Into::into))
            .transpose()
    }

    pub async fn list(&self, owner: UserId) -> Result<Vec<Job>, Error> {
        let projection = match &self.store {
            Store::Postgres(_) => "(data::jsonb - 'image')::text AS data",
            Store::Sqlite(_) => "json_remove(data, '$.image') AS data",
        };
        let jobs = self.query_jobs(&format!("SELECT {projection} FROM image_generation_jobs WHERE owner_id=$1 AND state NOT IN ('declined','dismissed','expired') ORDER BY updated_at DESC LIMIT 20"), &owner.to_string()).await?;
        let mut visible = Vec::new();
        for mut job in jobs {
            if self.published(&job).await? {
                job.transition("dismissed");
                job.image = None;
                job.prompt.clear();
                self.save(&mut job).await?;
            } else {
                visible.push(job);
            }
        }
        Ok(visible)
    }

    pub async fn published(&self, job: &Job) -> Result<bool, Error> {
        let Some(media) = &job.media else {
            return Ok(false);
        };
        let sql = "SELECT 1 FROM message_attachments WHERE cast(media_id AS TEXT)=$1 LIMIT 1";
        Ok(match &self.store {
            Store::Postgres(p) => sqlx::query(sql)
                .bind(media.id.to_string())
                .fetch_optional(p)
                .await?
                .is_some(),
            Store::Sqlite(p) => sqlx::query(sql)
                .bind(media.id.to_string())
                .fetch_optional(p)
                .await?
                .is_some(),
        })
    }

    async fn query_jobs(&self, sql: &str, arg: &str) -> Result<Vec<Job>, Error> {
        use sqlx::Row;
        let data: Vec<String> = match &self.store {
            Store::Postgres(pool) => sqlx::query(sql)
                .bind(arg)
                .fetch_all(pool)
                .await?
                .into_iter()
                .map(|r| r.get("data"))
                .collect(),
            Store::Sqlite(pool) => sqlx::query(sql)
                .bind(arg)
                .fetch_all(pool)
                .await?
                .into_iter()
                .map(|r| r.get("data"))
                .collect(),
        };
        data.into_iter()
            .map(|s| serde_json::from_str(&s).map_err(Into::into))
            .collect()
    }

    pub async fn enqueue(
        &self,
        owner: UserId,
        channel: crate::domain::ChannelId,
        request: Uuid,
        prompt: String,
    ) -> Result<Job, Error> {
        let id = Uuid::new_v5(
            &Uuid::NAMESPACE_URL,
            format!("sproyt:imagegen:{owner}:{request}").as_bytes(),
        )
        .to_string();
        if let Some(job) = self.get(&id).await? {
            if job.channel_id != channel || job.prompt != prompt {
                return Err("request id already used for a different image".into());
            }
            return Ok(job);
        }
        let job = Job {
            id,
            owner_id: owner.clone(),
            channel_id: channel.clone(),
            state: "queued".into(),
            prompt,
            created_at: now(),
            updated_at: now(),
            revision: 0,
            prompt_id: None,
            image: None,
            media: None,
            error: None,
        };
        let data = serde_json::to_string(&job)?;
        // Unique partial indexes enforce one unreviewed job per owner and at
        // most eight outstanding GPU requests, including across replicas.
        for slot in 1..=8 {
            let sql = "INSERT INTO image_generation_jobs(id,owner_id,channel_id,state,slot,updated_at,data) VALUES($1,$2,$3,'queued',$4,$5,$6) ON CONFLICT DO NOTHING";
            let inserted = match &self.store {
                Store::Postgres(p) => sqlx::query(sql)
                    .bind(&job.id)
                    .bind(owner.to_string())
                    .bind(channel.to_string())
                    .bind(slot)
                    .bind(now())
                    .bind(&data)
                    .execute(p)
                    .await?
                    .rows_affected(),
                Store::Sqlite(p) => sqlx::query(sql)
                    .bind(&job.id)
                    .bind(owner.to_string())
                    .bind(channel.to_string())
                    .bind(slot)
                    .bind(now())
                    .bind(&data)
                    .execute(p)
                    .await?
                    .rows_affected(),
            };
            if inserted == 1 {
                return Ok(job);
            }
        }
        if let Some(existing) = self.get(&job.id).await? {
            return Ok(existing);
        }
        Err("Review your current image first, or try again when the queue has room.".into())
    }

    pub async fn save(&self, job: &mut Job) -> Result<bool, Error> {
        let previous = job.revision;
        job.revision += 1;
        let data = serde_json::to_string(job)?;
        let sql = "UPDATE image_generation_jobs SET state=$1, updated_at=$2, revision=$3, data=$4 WHERE id=$5 AND revision=$6";
        let affected = match &self.store {
            Store::Postgres(p) => sqlx::query(sql)
                .bind(&job.state)
                .bind(job.updated_at)
                .bind(job.revision)
                .bind(&data)
                .bind(&job.id)
                .bind(previous)
                .execute(p)
                .await?
                .rows_affected(),
            Store::Sqlite(p) => sqlx::query(sql)
                .bind(&job.state)
                .bind(job.updated_at)
                .bind(job.revision)
                .bind(&data)
                .bind(&job.id)
                .bind(previous)
                .execute(p)
                .await?
                .rows_affected(),
        };
        Ok(affected == 1)
    }

    pub fn start_worker(&self, mut shutdown: watch::Receiver<bool>) {
        let this = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown.changed() => break,
                    _ = tokio::time::sleep(Duration::from_secs(3)) => {
                        if let Err(error) = this.tick().await { tracing::warn!(%error, "image generation worker failed"); }
                    }
                }
            }
        });
    }

    async fn tick(&self) -> Result<(), Error> {
        let lease = now() + 120;
        let sql =
            "UPDATE image_generation_worker SET lease_until=$1 WHERE id=1 AND lease_until < $2";
        let claimed = match &self.store {
            Store::Postgres(p) => sqlx::query(sql)
                .bind(lease)
                .bind(now())
                .execute(p)
                .await?
                .rows_affected(),
            Store::Sqlite(p) => sqlx::query(sql)
                .bind(lease)
                .bind(now())
                .execute(p)
                .await?
                .rows_affected(),
        };
        if claimed == 0 {
            return Ok(());
        }
        let result = self.work_one().await;
        let sql = "UPDATE image_generation_worker SET lease_until=0 WHERE id=1 AND lease_until=$1";
        match &self.store {
            Store::Postgres(p) => {
                sqlx::query(sql).bind(lease).execute(p).await?;
            }
            Store::Sqlite(p) => {
                sqlx::query(sql).bind(lease).execute(p).await?;
            }
        }
        result
    }

    async fn work_one(&self) -> Result<(), Error> {
        // Expired previews are removed, while a small tombstone keeps request
        // idempotency after a client reconnects with an old request id.
        let jobs = self.query_jobs("SELECT data FROM image_generation_jobs WHERE state IN ('queued','submitting','running','accepting') OR (state NOT IN ('expired','declined','dismissed') AND updated_at < cast($1 AS BIGINT)) ORDER BY updated_at LIMIT 8", &(now() - RETENTION).to_string()).await?;
        for mut job in jobs {
            if now() - job.updated_at > RETENTION {
                job.transition("expired");
                job.image = None;
                job.prompt.clear();
                job.media = None;
                self.save(&mut job).await?;
                continue;
            }
            if !matches!(
                job.state.as_str(),
                "queued" | "submitting" | "running" | "accepting"
            ) {
                continue;
            }
            if job.state == "accepting" {
                if now() - job.updated_at > 120 {
                    job.transition("ready");
                    self.save(&mut job).await?;
                }
                continue;
            }
            if job.state == "submitting" {
                job.transition("failed");
                job.error = Some("The server restarted during submission. The image may still be in ComfyUI; it was not queued again automatically.".into());
            } else if job.state == "queued" {
                job.transition("submitting");
                if !self.save(&mut job).await? {
                    continue;
                }
                match self.gateway.submit(&job).await {
                    Ok(id) => {
                        job.prompt_id = Some(id);
                        job.transition("running");
                    }
                    Err(_) => {
                        job.transition("failed");
                        job.error = Some("Could not confirm the request with Santorini. It has not been retried automatically.".into());
                    }
                }
            } else {
                match self.gateway.result(&job).await {
                    Ok(Some(image)) => {
                        job.image = Some(STANDARD.encode(image));
                        job.transition("ready");
                    }
                    Ok(None) if now() - job.updated_at <= 1800 => continue,
                    Ok(None) => {
                        job.transition("failed");
                        job.error = Some("Generation timed out after 30 minutes.".into());
                    }
                    Err(error) if error.to_string() == "ComfyUI generation failed" => {
                        job.transition("failed");
                        job.error = Some("Santorini could not generate this image. You can dismiss this request and try again.".into());
                    }
                    Err(_) if now() - job.updated_at <= 1800 => continue,
                    Err(_) => {
                        job.transition("failed");
                        job.error = Some("Could not retrieve the image from Santorini.".into());
                    }
                }
            }
            self.save(&mut job).await?;
            return Ok(());
        }
        Ok(())
    }
}

impl Gateway {
    async fn submit(&self, job: &Job) -> Result<String, Error> {
        let response: Value = self.http.post(format!("{}/prompt", self.base))
            .json(&json!({"prompt": workflow(&job.prompt, job.created_at as u64), "client_id":job.id}))
            .send().await?.error_for_status()?.json().await?;
        let id = response["prompt_id"].as_str().ok_or("missing prompt id")?;
        Uuid::parse_str(id)?;
        Ok(id.into())
    }

    async fn result(&self, job: &Job) -> Result<Option<Vec<u8>>, Error> {
        let id = job.prompt_id.as_deref().ok_or("missing prompt id")?;
        let history: Value = self
            .http
            .get(format!("{}/history/{id}", self.base))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let item = &history[id];
        if item.is_null() {
            return Ok(None);
        }
        if item["status"]["status_str"] == "error" {
            return Err("ComfyUI generation failed".into());
        }
        let image = &item["outputs"]["9"]["images"][0];
        let filename = image["filename"].as_str().ok_or("missing output")?;
        let subfolder = image["subfolder"].as_str().unwrap_or("");
        if image["type"] != "output" || !filename.ends_with(".png") {
            return Err("unexpected output".into());
        }
        let mut response = self
            .http
            .get(format!("{}/view", self.base))
            .query(&[
                ("filename", filename),
                ("subfolder", subfolder),
                ("type", "output"),
            ])
            .send()
            .await?
            .error_for_status()?;
        if response
            .content_length()
            .is_some_and(|n| n > MAX_IMAGE as u64)
        {
            return Err("image too large".into());
        }
        let mut content = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if content.len() + chunk.len() > MAX_IMAGE {
                return Err("image too large".into());
            }
            content.extend_from_slice(&chunk);
        }
        if !content.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Err("invalid image".into());
        }
        Ok(Some(content))
    }
}

pub(crate) fn workflow(prompt: &str, seed: u64) -> Value {
    json!({
        "1":{"class_type":"UNETLoader","inputs":{"unet_name":"flux1-dev-fp8.safetensors","weight_dtype":"default"}},
        "2":{"class_type":"DualCLIPLoader","inputs":{"clip_name1":"clip_l.safetensors","clip_name2":"t5xxl_fp16.safetensors","type":"flux","device":"default"}},
        "3":{"class_type":"VAELoader","inputs":{"vae_name":"ae.safetensors"}},
        "4":{"class_type":"CLIPTextEncode","inputs":{"text":prompt,"clip":["2",0]}},
        "5":{"class_type":"FluxGuidance","inputs":{"conditioning":["4",0],"guidance":3.5}},
        "6":{"class_type":"EmptySD3LatentImage","inputs":{"width":768,"height":768,"batch_size":1}},
        "7":{"class_type":"KSampler","inputs":{"model":["10",0],"positive":["5",0],"negative":["4",0],"latent_image":["6",0],"seed":seed,"steps":28,"cfg":1,"sampler_name":"euler","scheduler":"simple","denoise":1}},
        "8":{"class_type":"VAEDecode","inputs":{"samples":["7",0],"vae":["3",0]}},
        "9":{"class_type":"SaveImage","inputs":{"images":["8",0],"filename_prefix":"Sproyt-Heartsync"}},
        "10":{"class_type":"LoraLoaderModelOnly","inputs":{"model":["1",0],"lora_name":"Heartsync_Flux_NSFW_uncensored.safetensors","strength_model":1}}
    })
}

#[cfg(test)]
impl ImageGeneration {
    pub(crate) async fn test(base: &str) -> Self {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql(include_str!(
            "../migrations/sqlite/0035_image_generation.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::raw_sql("CREATE TABLE message_attachments(media_id TEXT)")
            .execute(&pool)
            .await
            .unwrap();
        Self {
            store: Store::Sqlite(pool),
            gateway: Arc::new(Gateway {
                base: base.into(),
                http: reqwest::Client::new(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ChannelId;

    #[tokio::test]
    async fn imagegen_postgres_queue_contract() {
        let Ok(url) = std::env::var("SPROYT_POSTGRES_TEST_URL") else {
            return;
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .unwrap();
        let schema = format!("imagegen_{}", Uuid::now_v7().simple());
        sqlx::raw_sql(&format!(
            "CREATE SCHEMA {schema}; SET search_path TO {schema};"
        ))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::raw_sql(include_str!(
            "../migrations/postgres/0035_image_generation.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();
        let service = ImageGeneration {
            store: Store::Postgres(pool.clone()),
            gateway: Arc::new(Gateway {
                base: "http://127.0.0.1:1".into(),
                http: reqwest::Client::new(),
            }),
        };
        let owner = UserId::named("postgres-image-owner");
        let channel = ChannelId::generate();
        let request = Uuid::now_v7();
        let mut job = service
            .enqueue(owner.clone(), channel.clone(), request, "sea".into())
            .await
            .unwrap();
        assert_eq!(
            service
                .enqueue(owner.clone(), channel, request, "sea".into())
                .await
                .unwrap()
                .id,
            job.id
        );
        assert_eq!(service.list(owner).await.unwrap().len(), 1);
        job.transition("submitting");
        assert!(service.save(&mut job).await.unwrap());
        service.tick().await.unwrap();
        assert_eq!(service.get(&job.id).await.unwrap().unwrap().state, "failed");
        sqlx::raw_sql(&format!(
            "SET search_path TO public; DROP SCHEMA {schema} CASCADE;"
        ))
        .execute(&pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn durable_admission_is_idempotent_bounded_and_owner_scoped() {
        let service = ImageGeneration::test("http://unused").await;
        let owner = UserId::named("alice");
        let channel = ChannelId::generate();
        let request = Uuid::now_v7();
        let job = service
            .enqueue(owner.clone(), channel.clone(), request, "seascape".into())
            .await
            .unwrap();
        let same = service
            .enqueue(owner.clone(), channel.clone(), request, "seascape".into())
            .await
            .unwrap();
        assert_eq!(job.id, same.id);
        assert!(
            service
                .enqueue(owner.clone(), channel.clone(), request, "different".into())
                .await
                .is_err()
        );
        assert!(
            service
                .enqueue(
                    owner.clone(),
                    channel.clone(),
                    Uuid::now_v7(),
                    "second".into()
                )
                .await
                .is_err()
        );
        for i in 1..8 {
            service
                .enqueue(
                    UserId::named(format!("user{i}")),
                    channel.clone(),
                    Uuid::now_v7(),
                    "sea".into(),
                )
                .await
                .unwrap();
        }
        assert!(
            service
                .enqueue(
                    UserId::named("ninth"),
                    channel.clone(),
                    Uuid::now_v7(),
                    "sea".into()
                )
                .await
                .is_err()
        );
        assert_eq!(service.list(owner).await.unwrap().len(), 1);
        assert!(
            service
                .list(UserId::named("outsider"))
                .await
                .unwrap()
                .is_empty()
        );
        // Only one competing decision may win, even from separate service clones.
        let mut first = job.clone();
        let mut stale = job;
        first.transition("ready");
        assert!(service.save(&mut first).await.unwrap());
        stale.transition("failed");
        assert!(!service.clone().save(&mut stale).await.unwrap());
        assert_eq!(
            service.get(&first.id).await.unwrap().unwrap().state,
            "ready"
        );
    }

    #[tokio::test]
    async fn worker_uses_fixed_workflow_and_recovers_ready_preview_from_database() {
        use axum::{
            Json, Router,
            routing::{get, post},
        };
        let captured = Arc::new(tokio::sync::Mutex::new(Value::Null));
        let sink = captured.clone();
        let id = Uuid::now_v7().to_string();
        let result_id = id.clone();
        let submit_id = id.clone();
        let app=Router::new().route("/prompt",post(move |Json(value):Json<Value>| {
            let sink=sink.clone();let id=submit_id.clone(); async move { *sink.lock().await=value; Json(json!({"prompt_id":id})) }
        })).route("/history/{id}",get(move || {let id=result_id.clone();async move {Json(json!({id:{"status":{"status_str":"success"},"outputs":{"9":{"images":[{"filename":"test.png","subfolder":"","type":"output"}]}}}}))}}))
        .route("/view",get(|| async { b"\x89PNG\r\n\x1a\nmock".to_vec() }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let service = ImageGeneration::test(&format!("http://{address}")).await;
        let job = service
            .enqueue(
                UserId::named("alice"),
                ChannelId::generate(),
                Uuid::now_v7(),
                "an oil painting of the sea".into(),
            )
            .await
            .unwrap();
        service.tick().await.unwrap();
        assert_eq!(
            service.get(&job.id).await.unwrap().unwrap().state,
            "running"
        );
        service.clone().tick().await.unwrap();
        let ready = service.get(&job.id).await.unwrap().unwrap();
        assert_eq!(ready.state, "ready");
        assert!(ready.image.is_some());
        assert!(ready.view().get("image").is_none());
        assert!(ready.media.is_none());
        let submitted = captured.lock().await;
        assert_eq!(submitted["prompt"]["4"]["inputs"]["text"], job.prompt);
        assert_eq!(
            submitted["prompt"]["10"]["inputs"]["lora_name"],
            "Heartsync_Flux_NSFW_uncensored.safetensors"
        );
        task.abort();
    }

    #[tokio::test]
    async fn interrupted_submission_is_not_duplicated_and_previews_expire() {
        let service = ImageGeneration::test("http://127.0.0.1:1").await;
        let mut job = service
            .enqueue(
                UserId::named("alice"),
                ChannelId::generate(),
                Uuid::now_v7(),
                "sea".into(),
            )
            .await
            .unwrap();
        job.transition("submitting");
        service.save(&mut job).await.unwrap();
        service.tick().await.unwrap();
        let mut recovered = service.get(&job.id).await.unwrap().unwrap();
        assert_eq!(recovered.state, "failed");
        recovered.transition("ready");
        recovered.image = Some("private".into());
        recovered.updated_at = now() - RETENTION - 1;
        service.save(&mut recovered).await.unwrap();
        service.tick().await.unwrap();
        let expired = service.get(&job.id).await.unwrap().unwrap();
        assert_eq!(expired.state, "expired");
        assert!(expired.image.is_none());
        assert!(expired.prompt.is_empty());
    }
}
