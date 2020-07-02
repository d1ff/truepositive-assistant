use hyper::Client;
use hyper_rustls::HttpsConnector;
use hyper_socks2::{Auth, SocksConnector};
use oauth2::basic::BasicClient;
use oauth2::{AuthUrl, ClientId, ClientSecret, RedirectUrl, TokenUrl};
use structopt::StructOpt;
use telegram_bot::connector::hyper::HyperConnector;
use telegram_bot::Api;
use url::Url;
use youtrack_rs::client::YouTrack;

use super::errors::*;

#[derive(StructOpt, Debug, Clone)]
#[structopt(name = "truepositive-assistant")]
pub struct BotOpt {
    #[structopt(long, env = "TELEGRAM_BOT_TOKEN")]
    pub telegram_token: String,

    #[structopt(long, env = "SOCKS5_PROXY")]
    pub socks5_proxy: Option<String>,

    #[structopt(long, env = "YOUTRACK_URL")]
    pub youtrack_url: String,

    #[structopt(long, env = "YOUTRACK_TOKEN")]
    pub youtrack_token: String,

    #[structopt(long, env = "BACKLOG_QUERY")]
    pub youtrack_backlog: String,

    #[structopt(long, env = "YOUTRACK_HUB_URL")]
    pub youtrack_hub: String,

    #[structopt(long, env = "YOUTRACK_CLIENTID")]
    pub youtrack_client_id: String,

    #[structopt(long, env = "YOUTRACK_CLIENTSECRET")]
    pub youtrack_client_secret: String,

    #[structopt(long, env = "AUTH_CALLBACK_URL")]
    pub auth_callback_url: String,
}

impl BotOpt {
    pub fn telegram_api(&self) -> Api {
        match &self.socks5_proxy {
            Some(socks5_proxy) => {
                let auth = {
                    let url = Url::parse(&socks5_proxy).expect("Invalid proxy url");
                    let username = url.username();
                    if let Some(password) = url.password() {
                        Some(Auth::new(username, password))
                    } else {
                        None
                    }
                };

                let connector = HttpsConnector::new();
                let proxy = SocksConnector {
                    proxy_addr: socks5_proxy.parse().expect("Could not parse proxy url"),
                    auth,
                    connector,
                };
                let proxy = proxy.with_tls().unwrap();
                let connector = Box::new(HyperConnector::new(Client::builder().build(proxy)));
                Api::with_connector(self.telegram_token.clone(), connector)
            }
            None => Api::new(self.telegram_token.clone()),
        }
    }

    pub fn youtrack_api(&self) -> Result<YouTrack> {
        YouTrack::new(self.youtrack_url.clone(), self.youtrack_token.clone()).map_err(|e| e.into())
    }

    pub fn oauth_client(&self) -> oauth2::basic::BasicClient {
        let auth_url = AuthUrl::new(format!("{}/api/rest/oauth2/auth", self.youtrack_hub))
            .expect("Invalid authorization endpoint URL");
        let token_url = TokenUrl::new(format!("{}/api/rest/oauth2/token", self.youtrack_hub))
            .expect("Invalid token endpoint URL");

        BasicClient::new(
            ClientId::new(self.youtrack_client_id.clone()),
            Some(ClientSecret::new(self.youtrack_client_secret.clone())),
            auth_url,
            Some(token_url),
        )
        .set_redirect_url(
            RedirectUrl::new(self.auth_callback_url.clone()).expect("Invalid redirect url"),
        )
    }
}
