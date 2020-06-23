use super::errors::*;

use actix_web::{dev::Server, middleware, web, App, HttpRequest, HttpResponse, HttpServer};
use oauth2::basic::BasicClient;
use oauth2::{AuthorizationCode, CsrfToken};
use serde::Deserialize;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use youtrack_rs::client::{Executor, YouTrack};

use super::bot::Bot;

#[derive(Clone)]
struct AppState {
    bot: Arc<Mutex<Box<Bot>>>,
}

#[derive(Deserialize, Clone)]
pub struct AuthRequest {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: String,
    pub scope: String,
    pub state: String,
}

fn auth(data: web::Data<AppState>) -> HttpResponse {
    let html = format!(
        r#"<html>
        <head><title>OAuth2 Test</title></head>
        <script>
            var new_q = location.hash.substr(1);
            window.location.href = location.origin + "/auth2?" + new_q;
        </script>
        <body>
        </body>
    </html>"#
    );
    HttpResponse::Ok().body(html)
}

fn auth2(data: web::Data<AppState>, params: web::Query<AuthRequest>) -> HttpResponse {
    let mut bot = data.bot.lock().unwrap();
    bot.on_auth(params.clone());

    let html = format!(
        r#"<html>
        <head><title>OAuth2 Test</title></head>
        <body>
        <h1>You may close this window</h1>
        </body>
    </html>"#
    );
    HttpResponse::Ok().body(html)
}

async fn index(req: HttpRequest) -> &'static str {
    println!("REQ: {:?}", req);
    "Hello, world!"
}

pub fn run(bot: Arc<Mutex<Box<Bot>>>) -> Result<Server> {
    Ok(HttpServer::new(move || {
        let data = AppState { bot: bot.clone() };
        App::new()
            .data(data)
            .wrap(middleware::Logger::default())
            .route("/", web::get().to(index))
            .route("/auth", web::get().to(auth))
            .route("/auth2", web::get().to(auth2))
    })
    .bind("0.0.0.0:5000")?
    .run())
}
