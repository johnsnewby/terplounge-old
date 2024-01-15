use askama::Template; // bring trait in scope

use crate::session::{get_sessions, user_closing, user_connected, SessionData};
use crate::translate;

use crossbeam_channel::Sender;
use rust_embed::RustEmbed;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use warp::reply::Json;
use warp::Filter;

#[derive(Template)]
#[template(path = "index.html", escape = "none")]
pub struct Index {
    sessions: Vec<SessionData>,
}

pub async fn index() -> std::result::Result<impl warp::Reply, warp::Rejection> {
    let mut sessions = get_sessions().await.ok_or(warp::reject::reject())?;
    sessions.sort_by(|a, b| {
        a.created_at
            .partial_cmp(&b.created_at)
            .expect("Unexpected error in comparison")
    });

    let template = Index { sessions };

    Ok(warp::reply::html(template.render().unwrap()))
}

pub async fn serve(translate_tx: Sender<translate::TranslationRequest>) {
    let chat = warp::path("chat")
        .and(warp::query::<HashMap<String, String>>())
        .and(warp::ws())
        .map(move |params: HashMap<String, String>, ws: warp::ws::Ws| {
            let tx = translate_tx.clone();
            let lang: String = (params.get("lang").unwrap_or(&"de".to_string())).clone();

            let sample_rate: u32 = match params.get("rate") {
                Some(rate) => rate.to_string(),
                None => "44100".to_string(),
            }
            .parse()
            .unwrap();
            ws.on_upgrade(move |socket| user_connected(socket, tx.clone(), lang, sample_rate))
        });

    let close = warp::post().and(warp::path!("close" / String).and_then(async move |uuid| {
        user_closing(uuid).await;
        Ok::<&str, warp::Rejection>("foo")
    }));

    let status = warp::path!("status" / String).and_then(async move |uuid| {
        match crate::session::find_session_with_uuid(&uuid).await {
            Some(session_id) => match crate::session::get_session(&session_id).await {
                Some(session) => Ok::<Json, warp::Rejection>(warp::reply::json(&session)),
                None => Err(warp::reject::not_found()),
            },
            None => Err(warp::reject::not_found()),
        }
    });

    let compare = warp::get()
        .and(warp::path!("compare" / String / String / String))
        .and_then(async move |asset_id, uuid, lang| {
            match crate::compare::compare(asset_id, uuid, lang).await {
                Ok(x) => Ok(x),
                Err(e) => {
                    log::error!("Error in compare: {:?}", e);
                    Err(warp::reject())
                }
            }
        });

    let recordings_dir = std::env::var("RECORDINGS_DIR").unwrap_or("../clients/assets".to_string());

    let recordings = warp::get()
        .and(warp::path("recordings"))
        .and(warp::fs::dir(recordings_dir));

    let transcript = warp::path!("transcript" / String).and_then(async move |uuid| {
        match crate::session::find_session_with_uuid(&uuid).await {
            Some(session_id) => match crate::session::get_session(&session_id).await {
                Some(session) => Ok(session.transcript().unwrap()),
                None => Err(warp::reject::not_found()),
            },
            None => Err(warp::reject::not_found()),
        }
    });

    let index = warp::path::end().and_then(async move || crate::api::index().await);

    #[derive(RustEmbed)]
    #[folder = "../client"]
    struct StaticContent;
    let static_content_serve = warp_embed::embed(&StaticContent);

    #[derive(RustEmbed)]
    #[folder = "../client/assets"]
    struct Assets;
    let assets_serve = warp::path("assets").and(warp_embed::embed(&Assets));

    let routes = index
        .or(assets_serve)
        .or(chat)
        .or(close)
        .or(compare)
        .or(recordings)
        .or(status)
        .or(static_content_serve)
        .or(transcript);
    log::debug!("Starting server");
    let listen;
    if let Ok(x) = std::env::var(" LISTEN") {
        listen = x.parse().unwrap();
    } else {
        listen = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 3030);
    };

    warp::serve(routes).run(listen).await;
}
