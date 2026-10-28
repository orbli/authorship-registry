//! Outbound mail: console (dev/tests, records what it would send), SES, or any
//! SMTP relay such as Gmail with an app password. The SMTP password is read
//! lazily from env or once from an SSM SecureString.

use std::sync::{Arc, Mutex};

use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use tokio::sync::OnceCell;

use crate::config::{Config, MailDriver};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mail {
    pub to: String,
    pub subject: String,
    pub text: String,
}

#[derive(Clone)]
pub enum Mailer {
    Console(Arc<Mutex<Vec<Mail>>>),
    Ses { region: String, sender: String, client: Arc<OnceCell<aws_sdk_sesv2::Client>> },
    Smtp { cfg: Arc<Config>, transport: Arc<OnceCell<AsyncSmtpTransport<Tokio1Executor>>> },
}

impl Mailer {
    pub fn new(cfg: &Config) -> Mailer {
        match cfg.mail_driver {
            MailDriver::Console => Mailer::console(),
            MailDriver::Ses => Mailer::Ses { region: cfg.aws_region.clone(), sender: cfg.sender_email.clone(), client: Arc::default() },
            MailDriver::Smtp => Mailer::Smtp { cfg: Arc::new(cfg.clone()), transport: Arc::default() },
        }
    }

    pub fn console() -> Mailer {
        Mailer::Console(Arc::default())
    }

    /// Console mailer only: everything "sent" so far.
    pub fn sent(&self) -> Vec<Mail> {
        match self {
            Mailer::Console(v) => v.lock().unwrap().clone(),
            _ => Vec::new(),
        }
    }

    pub async fn send(&self, mail: Mail) -> anyhow::Result<()> {
        match self {
            Mailer::Console(v) => {
                println!("\n--- mail to {} ---\n{}\n\n{}\n---\n", mail.to, mail.subject, mail.text);
                v.lock().unwrap().push(mail);
                Ok(())
            }
            Mailer::Ses { region, sender, client } => {
                let client = client
                    .get_or_init(|| async {
                        let sdk = aws_config::defaults(aws_config::BehaviorVersion::latest()).region(aws_config::Region::new(region.clone())).load().await;
                        aws_sdk_sesv2::Client::new(&sdk)
                    })
                    .await;
                use aws_sdk_sesv2::types::{Body, Content, Destination, EmailContent, Message as SesMessage};
                let content = |s: &str| Content::builder().data(s).charset("UTF-8").build().expect("content");
                client
                    .send_email()
                    .from_email_address(sender)
                    .destination(Destination::builder().to_addresses(&mail.to).build())
                    .content(EmailContent::builder().simple(SesMessage::builder().subject(content(&mail.subject)).body(Body::builder().text(content(&mail.text)).build()).build()).build())
                    .send()
                    .await?;
                Ok(())
            }
            Mailer::Smtp { cfg, transport } => {
                let t = transport
                    .get_or_try_init(|| async {
                        let pass = match (&cfg.smtp.password, &cfg.smtp.password_param) {
                            (Some(p), _) => p.clone(),
                            (None, Some(param)) => ssm_parameter(&cfg.aws_region, param).await?,
                            (None, None) => anyhow::bail!("SMTP password unavailable"),
                        };
                        let builder =
                            if cfg.smtp.port == 465 { AsyncSmtpTransport::<Tokio1Executor>::relay(&cfg.smtp.host)? } else { AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&cfg.smtp.host)? };
                        Ok::<_, anyhow::Error>(builder.port(cfg.smtp.port).credentials(Credentials::new(cfg.smtp.user.clone(), pass)).build())
                    })
                    .await?;
                let msg = Message::builder().from(cfg.sender_email.parse()?).to(mail.to.parse()?).subject(&mail.subject).body(mail.text.clone())?;
                t.send(msg).await?;
                Ok(())
            }
        }
    }
}

/// Read a (decrypted) SSM parameter value.
pub async fn ssm_parameter(region: &str, name: &str) -> anyhow::Result<String> {
    let sdk = aws_config::defaults(aws_config::BehaviorVersion::latest()).region(aws_config::Region::new(region.to_string())).load().await;
    let r = aws_sdk_ssm::Client::new(&sdk).get_parameter().name(name).with_decryption(true).send().await?;
    r.parameter.and_then(|p| p.value).ok_or_else(|| anyhow::anyhow!("SSM parameter {name} has no value"))
}
