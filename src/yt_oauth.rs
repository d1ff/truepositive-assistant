use super::errors::*;

use actix_web::http::header;
use actix_web::{dev::Server, middleware, web, App, HttpRequest, HttpResponse, HttpServer};
use oauth2::basic::BasicClient;
use oauth2::reqwest::http_client;
use oauth2::{
    AccessToken, AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, PkceCodeChallenge,
    RedirectUrl, Scope, TokenResponse, TokenUrl,
};
use serde::Deserialize;
use serde_json::Value;
use youtrack_rs::client::{Executor, YouTrack};

struct AppState {
    oauth: BasicClient,
}

fn login(data: web::Data<AppState>) -> HttpResponse {
    // Create a PKCE code verifier and SHA-256 encode it as a code challenge.
    let (pkce_code_challenge, _pkce_code_verifier) = PkceCodeChallenge::new_random_sha256();
    // Generate the authorization URL to which we'll redirect the user.
    let (auth_url, _csrf_token) = &data
        .oauth
        .authorize_url(CsrfToken::new_random)
        // Set the desired scopes.
        .add_scope(Scope::new("YouTrack".to_string()))
        // Set the PKCE code challenge.
        .set_pkce_challenge(pkce_code_challenge)
        .use_implicit_flow()
        .url();

    HttpResponse::Found()
        .header(header::LOCATION, auth_url.to_string())
        .finish()
}

#[derive(Deserialize)]
struct AuthRequest {
    access_token: String,
    token_type: String,
    expires_in: String,
    scope: String,
    state: String,
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
    let code = AuthorizationCode::new(params.access_token.clone());
    let _state = CsrfToken::new(params.state.clone());

    let yt_url = "https://truepositive.myjetbrains.com/youtrack/api/";
    let yt =
        YouTrack::new(yt_url, params.access_token.as_str()).expect("could not create youtrack");
    let issues = yt
        .get()
        .issues()
        .query("#unresolved")
        .top("10")
        .skip("0")
        .fields("idReadable,summary,votes,voters(hasVote)")
        .execute::<Value>();
    match issues {
        Ok((headers, status, json)) => {
            println!("{:#?}", headers);
            println!("{}", status);
            println!("{:?}", json);
        }
        Err(_) => {}
    };
    let html = format!(
        r#"<html>
        <head><title>OAuth2 Test</title></head>
        <script>
            window.close();
        </script>
        <body>
        </body>
    </html>"#
    );
    HttpResponse::Ok().body(html)
}

async fn index(req: HttpRequest) -> &'static str {
    println!("REQ: {:?}", req);
    "Hello, world!"
}

use super::opts::*;

pub fn run(opt: BotOpt) -> Result<Server> {
    Ok(HttpServer::new(move || {
        let auth_url = AuthUrl::new(format!("{}/api/rest/oauth2/auth", opt.youtrack_hub))
            .expect("Invalid authorization endpoint URL");
        let token_url = TokenUrl::new(format!("{}/api/rest/oauth2/token", opt.youtrack_hub))
            .expect("Invalid token endpoint URL");

        let client = BasicClient::new(
            ClientId::new(opt.youtrack_client_id.clone()),
            Some(ClientSecret::new(opt.youtrack_client_secret.clone())),
            auth_url,
            Some(token_url),
        )
        .set_redirect_url(
            RedirectUrl::new("http://127.0.0.1:5000/auth".to_string())
                .expect("Invalid redirect url"),
        );
        App::new()
            .data(AppState { oauth: client })
            .wrap(middleware::Logger::default())
            .route("/", web::get().to(index))
            .route("/login", web::get().to(login))
            .route("/auth", web::get().to(auth))
            .route("/auth2", web::get().to(auth2))
    })
    .bind("127.0.0.1:5000")?
    .run())
}
