use bb8_redis::{bb8, redis, RedisConnectionManager};
use futures::{FutureExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::prelude::*;
use tokio::sync::{mpsc, RwLock};
use warp::ws::{Message, WebSocket};
use warp::Filter;

#[derive(Serialize, Deserialize)]
pub struct ChatMessage {
    user_id: usize,
    channel: String,
    content: String,
}

static INDEX_HTML: &str = "<h1>Hello</h1>";

type Users = Arc<RwLock<HashMap<usize, mpsc::UnboundedSender<Result<Message, warp::Error>>>>>;

async fn get_user_id(pool: bb8::Pool<RedisConnectionManager>) -> usize {
    let mut conn = pool.get().await.unwrap();
    let reply: usize = redis::cmd("INCR")
    .arg("NEXT_USER_ID")
    .query_async(&mut *conn)
    .await
    .unwrap();
    println!("NEXT_USER_ID: {:?}", reply);
    return reply;
}

#[tokio::main]
async fn main() {
    pretty_env_logger::init();
    let users = Users::default();

    let manager = RedisConnectionManager::new("redis://localhost").unwrap();
    let pool = bb8::Pool::builder().build(manager).await.unwrap();

    initialise_subscriptions(users.clone(), pool.clone());

    let users = warp::any().map(move || users.clone());

    let pool = warp::any().map(move || pool.clone());

    let chat = warp::path("chat").and(warp::ws()).and(users).and(pool).map(
        |ws: warp::ws::Ws, users, pool| {
            ws.on_upgrade(move |socket| user_connected(socket, users, pool))
        },
    );
    let index = warp::path::end().map(|| warp::reply::html(INDEX_HTML));

    let routes = index.or(chat);

    let port: u16 = env::var("PORT").unwrap().parse::<u16>().unwrap();

    warp::serve(routes).run(([127, 0, 0, 1], port)).await;
}

// This is the main bit here
fn initialise_subscriptions(users: Users, pool: bb8::Pool<RedisConnectionManager>) {
    tokio::task::spawn(async move {
        println!("Started task");
        let conn = bb8::Pool::dedicated_connection(&pool).await.unwrap();
        let mut pubsub = conn.into_pubsub();
        let subscribed = pubsub.subscribe("Chat 1").await;
        println!("Subscribed Response: {:?}", subscribed);
        while let Some(result) = pubsub.on_message().next().await {
            let payload = result.get_payload::<String>().unwrap();
            let received_message: ChatMessage =
                serde_json::from_str(&String::from(payload)).unwrap();
            let text = format!(
                "User {}: {}",
                received_message.user_id, received_message.content
            );
            println!("<MSG>: {}", text);
            for (&uid, tx) in users.read().await.iter() {
                if uid != received_message.user_id {
                    if let Err(_disconnected) = tx.send(Ok(Message::text(&text))) {
                    }
                }
            }
        }
    });
}

async fn user_connected(ws: WebSocket, users: Users, pool: bb8::Pool<RedisConnectionManager>) {
    // Use a counter to assign a new unique ID for this user.
    
    let my_id = get_user_id(pool.clone()).await;

    eprintln!("new chat user: {}", my_id);

    // Split the socket into a sender and receive of messages.
    let (user_ws_tx, mut user_ws_rx) = ws.split();

    // Use an unbounded channel to handle buffering and flushing of messages
    // to the websocket...
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::task::spawn(rx.forward(user_ws_tx).map(|result| {
        if let Err(e) = result {
            eprintln!("websocket send error: {}", e);
        }
    }));

    // Save the sender in our list of connected users.
    users.write().await.insert(my_id, tx);

    // Return a `Future` that is basically a state machine managing
    // this specific user's connection.

    // Make an extra clone to give to our disconnection handler...
    let users2 = users.clone();

    // Every time the user sends a message, broadcast it to
    // all other users...
    while let Some(result) = user_ws_rx.next().await {
        let msg = match result {
            Ok(msg) => msg,
            Err(e) => {
                eprintln!("websocket error(uid={}): {}", my_id, e);
                break;
            }
        };
        user_message(my_id, msg, &pool).await;
    }

    // user_ws_rx stream will keep processing as long as the user stays
    // connected. Once they disconnect, then...
    user_disconnected(my_id, &users2).await;
}

async fn user_message(
    my_id: usize,
    msg: Message,
    pool: &bb8::Pool<RedisConnectionManager>,
) {
    // Skip any non-Text messages...
    let msg = if let Ok(s) = msg.to_str() {
        s
    } else {
        return;
    };

    let newer_msg = ChatMessage {
        user_id: my_id,
        content: msg.to_string(),
        channel: "Chat 1".to_string(),
    };
    // Send off to Redis
    let mut conn = pool.get().await.unwrap();
    let reply: i32 = redis::cmd("PUBLISH")
        .arg("Chat 1")
        .arg(serde_json::to_string(&newer_msg).unwrap())
        .query_async(&mut *conn)
        .await
        .unwrap();
    println!("Pub response: {:?}", reply);
}

async fn user_disconnected(my_id: usize, users: &Users) {
    eprintln!("good bye user: {}", my_id);

    // Stream closed up, so remove from the user list
    users.write().await.remove(&my_id);
}
