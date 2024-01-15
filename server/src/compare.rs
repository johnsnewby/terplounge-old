use askama::Template; // bring trait in scope
use std::fs;

use crate::session::find_session_with_uuid;

#[derive(Template)]
#[template(path = "compare.html", escape = "none")]
pub struct Comparison {
    source: String,
    dest: String,
}

fn escape(from: String) -> String {
    from.replace('\'', "\\'")
        .replace('\n', "\\\n")
        .replace('\"', "\\\"")
}

pub async fn compare(
    asset_id: String,
    uuid: String,
    lang: String,
) -> std::result::Result<impl warp::Reply, warp::Rejection> {
    let source = match fs::read_to_string(format!("../client/assets/{}/{}.txt", asset_id, lang)) {
        Ok(x) => escape(x),
        Err(_) => return Err(warp::reject::not_found()),
    };

    let session_id = find_session_with_uuid(&uuid)
        .await
        .ok_or(warp::reject::not_found())?;

    let session = crate::session::get_session(&session_id)
        .await
        .ok_or(warp::reject::not_found())?;

    let dest = match session.transcript() {
        Ok(e) => escape(e.to_string()),
        Err(e) => {
            log::error!("Couldn't get transcript for uuid {}: {:?}", uuid, e);
            return Err(warp::reject::reject());
        }
    };

    let template = Comparison { source, dest };

    Ok(warp::reply::html(template.render().unwrap()))
}
