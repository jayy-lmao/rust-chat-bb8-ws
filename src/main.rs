use bb8_redis::{bb8, redis, RedisConnectionManager};
use futures::{FutureExt, StreamExt};
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

static NEXT_USER_ID: AtomicUsize = AtomicUsize::new(1);

static INDEX_HTML: &str = "<h1>Hello</h1>";

type Users = Arc<RwLock<HashMap<usize, mpsc::UnboundedSender<Result<Message, warp::Error>>>>>;

#[tokio::main]
async fn main() {
    pretty_env_logger::init();
    let users = Users::default();

    let manager = RedisConnectionManager::new("redis://localhost").unwrap();
    let pool = bb8::Pool::builder().build(manager).await.unwrap();

    // let (rx, tx) = mpsc::unbounded_channel();
    {
        // This is the task which will redirect subscriptions into (rx, tx)
        let pool = pool.clone();
        tokio::task::spawn(async move {
            println!("Started task");
            let conn = bb8::Pool::dedicated_connection(&pool).await.unwrap();
            let mut pubsub = conn.into_pubsub();
            let subscribed = pubsub.subscribe("Chat 1").await;
            println!("subbed: {:?}", subscribed);
            while let Some(result) = pubsub.on_message().next().await {
                println!("{:?}", result);
            };

            // I want to feed each message into (tx)

        });
    };

    // What on earth is this doing
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

async fn user_connected(ws: WebSocket, users: Users, pool: bb8::Pool<RedisConnectionManager>) {
    // Use a counter to assign a new unique ID for this user.
    let my_id = NEXT_USER_ID.fetch_add(1, Ordering::Relaxed);

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
        user_message(my_id, msg, &users, &pool).await;
    }

    // user_ws_rx stream will keep processing as long as the user stays
    // connected. Once they disconnect, then...
    user_disconnected(my_id, &users2).await;
}

async fn user_message(my_id: usize, msg: Message, users: &Users, pool: &bb8::Pool<RedisConnectionManager>) {
    // Skip any non-Text messages...
    let msg = if let Ok(s) = msg.to_str() {
        s
    } else {
        return;
    };

    let new_msg = format!("<User#{}>: {}", my_id, msg);

    // Send off to Redis
    let mut conn = pool.get().await.unwrap();
    let reply: String = redis::cmd("PUBLISH")
        .arg("Chat 1")
        .arg("Hi")
        .query_async(&mut *conn)
        .await
        .unwrap();
    println!("pub: {:?}", reply);

    // New message from this user, send it to everyone else (except same uid)...
    for (&uid, tx) in users.read().await.iter() {
        if my_id != uid {
            if let Err(_disconnected) = tx.send(Ok(Message::text(new_msg.clone()))) {
                // The tx is disconnected, our `user_disconnected` code
                // should be happening in another task, nothing more to
                // do here.
            }
        }
    }
}

async fn user_disconnected(my_id: usize, users: &Users) {
    eprintln!("good bye user: {}", my_id);

    // Stream closed up, so remove from the user list
    users.write().await.remove(&my_id);
}
