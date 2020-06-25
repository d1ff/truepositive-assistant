use super::errors::*;

use actix_web::{dev::Server, middleware, web, App, HttpResponse, HttpServer};
use serde::Deserialize;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tera::Context;

use super::bot::Bot;

#[derive(Clone)]
struct AppState {
    bot: Arc<Mutex<Box<Bot>>>,
}

#[derive(Deserialize, Clone)]
pub struct AuthRequest {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: u64,
    pub scope: String,
    pub state: String,
}

impl AuthRequest {
    pub fn expires_in_duration(&self) -> Duration {
        Duration::from_secs(self.expires_in)
    }
}

fn auth(data: web::Data<AppState>) -> HttpResponse {
    let bot = data.bot.lock().unwrap();
    let context = Context::new();
    let html = bot.templates.render("auth.html", &context);
    HttpResponse::Ok().body(html.unwrap())
}

fn auth2(data: web::Data<AppState>, params: web::Query<AuthRequest>) -> HttpResponse {
    let mut bot = data.bot.lock().unwrap();
    bot.on_auth(params.clone());

    let context = Context::new();
    let html = bot.templates.render("auth2.html", &context);
    HttpResponse::Ok().body(html.unwrap())
}

pub fn run(bot: Arc<Mutex<Box<Bot>>>) -> Result<Server> {
    Ok(HttpServer::new(move || {
        let data = AppState { bot: bot.clone() };
        App::new()
            .data(data)
            .wrap(middleware::Logger::default())
            .route("/auth", web::get().to(auth))
            .route("/auth2", web::get().to(auth2))
    })
    .bind("0.0.0.0:5000")?
    .run())
}
