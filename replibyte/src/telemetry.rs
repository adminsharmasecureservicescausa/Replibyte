use crate::config::{ConnectionUri, TransformerTypeConfig};
use crate::{BackupCommand, Config, RestoreCommand, SubCommand, TransformerCommand};
use chrono::{NaiveDateTime, Utc};
use reqwest::blocking::Client as HttpClient;
use reqwest::header::CONTENT_TYPE;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fmt::format;
use std::io::{Error, ErrorKind};
use std::time::Duration;

extern crate serde_json;

pub const TELEMETRY_TOKEN: &str = "phc_3I35toj7Gbkiz5YZdxt2h5KOWBEfRx17qLCZ2OWj5Bt";
const API_ENDPOINT: &str = "https://app.posthog.com/capture/";
const TIMEOUT: &Duration = &Duration::from_millis(3000);

pub struct ClientOptions {
    api_endpoint: String,
    api_key: String,
}

impl From<&str> for ClientOptions {
    fn from(api_key: &str) -> Self {
        ClientOptions {
            api_endpoint: API_ENDPOINT.to_string(),
            api_key: api_key.to_string(),
        }
    }
}

pub struct TelemetryClient {
    options: ClientOptions,
    client: HttpClient,
}

impl TelemetryClient {
    pub fn new<C: Into<ClientOptions>>(options: C) -> Self {
        let client = HttpClient::builder()
            .timeout(Some(TIMEOUT.clone()))
            .build()
            .unwrap(); // Unwrap here is as safe as `HttpClient::new`
        TelemetryClient {
            options: options.into(),
            client,
        }
    }

    pub fn capture(&self, event: Event) -> Result<(), Error> {
        let inner_event = InnerEvent::new(event, self.options.api_key.clone());
        let _res = self
            .client
            .post(self.options.api_endpoint.clone())
            .header(CONTENT_TYPE, "application/json")
            .body(serde_json::to_string(&inner_event).expect("unwrap here is safe"))
            .send()
            .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
        Ok(())
    }

    pub fn capture_batch(&self, events: Vec<Event>) -> Result<(), Error> {
        for event in events {
            self.capture(event)?;
        }
        Ok(())
    }

    pub fn capture_command(
        &self,
        config: &Config,
        sub_command: &SubCommand,
        args: &Vec<String>,
        execution_time_in_millis: Option<u128>,
    ) -> Result<(), Error> {
        let mut props = HashMap::new();
        let _ = props.insert("args".to_string(), args.join(" ").to_string());

        match &config.source {
            Some(x) => {
                props.insert(
                    "database".to_string(),
                    match x.connection_uri()? {
                        ConnectionUri::Postgres(_, _, _, _, _) => "postgresql",
                        ConnectionUri::Mysql(_, _, _, _, _) => "mysql",
                        ConnectionUri::MongoDB(_, _, _, _, _, _) => "mongodb",
                    }
                    .to_string(),
                );

                props.insert(
                    "compression_used".to_string(),
                    x.compression.unwrap_or(true).to_string(),
                );

                props.insert(
                    "encryption_used".to_string(),
                    x.encryption_key.is_some().to_string(),
                );

                props.insert("skip_tables_used".to_string(), x.skip.is_some().to_string());

                props.insert(
                    "subset_used".to_string(),
                    x.database_subset.is_some().to_string(),
                );

                let mut transformers = HashSet::new();

                for transformer in &x.transformers {
                    for column in &transformer.columns {
                        transformers.insert(match column.transformer {
                            TransformerTypeConfig::Random => "random",
                            TransformerTypeConfig::RandomDate => "random-date",
                            TransformerTypeConfig::FirstName => "first-name",
                            TransformerTypeConfig::Email => "email",
                            TransformerTypeConfig::KeepFirstChar => "keep-first-char",
                            TransformerTypeConfig::PhoneNumber => "phone-number",
                            TransformerTypeConfig::CreditCard => "credit-card",
                            TransformerTypeConfig::Redacted(_) => "redacted",
                            TransformerTypeConfig::Transient => "transient",
                            TransformerTypeConfig::CustomWasm(_) => "custom-wasm",
                        });
                    }
                }

                for (idx, transformer_name) in transformers.iter().enumerate() {
                    props.insert(format!("transformer_{}", idx), transformer_name.to_string());
                }
            }
            None => {}
        };

        let event = match sub_command {
            SubCommand::Backup(cmd) => match cmd {
                BackupCommand::List => "backup-list",
                BackupCommand::Run(_) => "backup-run",
            },
            SubCommand::Transformer(cmd) => match cmd {
                TransformerCommand::List => "transformer-list",
            },
            SubCommand::Restore(cmd) => match cmd {
                RestoreCommand::Local(_) => "restore-local",
                RestoreCommand::Remote(_) => "restore-remote",
            },
        };

        self.capture(Event {
            event: event.to_string(),
            properties: Properties {
                distinct_id: machine_uid::get().unwrap_or("unknown".to_string()),
                props,
            },
            timestamp: Some(Utc::now().naive_utc()),
        })
    }
}

// This exists so that the client doesn't have to specify the API key over and over
#[derive(Serialize)]
struct InnerEvent {
    api_key: String,
    event: String,
    properties: Properties,
    timestamp: Option<NaiveDateTime>,
}

impl InnerEvent {
    fn new(event: Event, api_key: String) -> Self {
        Self {
            api_key,
            event: event.event,
            properties: event.properties,
            timestamp: event.timestamp,
        }
    }
}

pub struct Event {
    pub event: String,
    pub properties: Properties,
    pub timestamp: Option<NaiveDateTime>,
}

#[derive(Serialize)]
pub struct Properties {
    pub distinct_id: String,
    pub props: HashMap<String, String>,
}
