# Rust bb8/websocket/Redis Pubsub demo

This is an example of how you might run a Rust server, using Redis as Pubsub.

The value of this is being able to run multiple Rust websocket servers, which are able to exchange messages via the Redis Pubsub so that a user connected to server A can receive messages from a user connected to server B.

## Running
To run this, just have a Redis server running on port 6379 on your local machine, then:
```sh
export PORT=3030
cargo run
```
and if you want to run a separate server, to observe the servers communicating via the Pubsub just run another instance with a different port.
```sh
export PORT=3031
cargo run
```

## Testing

Personally, I like to copy the code from https://blog.scottlogic.com/2019/07/23/Testing-WebSockets-for-beginners.html and use it in my browser console.
Be warned, in the tutorial the snippet
```js
ws.onmessage = function (e) {
console.log("From Server:"+ e.data"); <=== this last " is a typo
};
```

_*Disclaimer: There are a lot of unwraps here, consider handling these errors if you use this code as inspo/boilerplate*_

