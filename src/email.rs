use lettre::transport::smtp;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Config {
    smtp_url: String,
    from: lettre::message::Mailbox,
    to: lettre::message::Mailbox,
}

pub trait Notifier: Send + Sync {
    fn notify(&self, title: &str, body: &str);
}

pub struct NoOpNotifier;

impl Notifier for NoOpNotifier {
    fn notify(&self, title: &str, _body: &str) {
        eprintln!("Notifications disabled; skipping sending notification with title {title}");
    }
}

pub struct Client {
    config: Config,
    transport: smtp::SmtpTransport,
}

impl Client {
    pub fn new(config: Config) -> Self {
        let transport = smtp::SmtpTransport::from_url(&config.smtp_url)
            .unwrap()
            .build();
        if !transport.test_connection().unwrap() {
            panic!("failed to connect")
        }
        Self { config, transport }
    }
}

impl Notifier for Client {
    fn notify(&self, title: &str, body: &str) {
        use lettre::message::header::ContentType;
        use lettre::Message;
        use lettre::Transport;
        eprintln!("Sending email with title {title}");
        let email = Message::builder()
            .from(self.config.from.clone())
            .to(self.config.to.clone())
            .subject(title)
            .header(ContentType::TEXT_PLAIN)
            .body(body.to_string())
            .unwrap();
        match self.transport.send(&email) {
            Ok(_) => eprintln!("Email sent successfully!"),
            Err(err) => eprintln!("Failed to send email: {err:?}"),
        }
    }
}
