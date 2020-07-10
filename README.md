# Rust bb8/websocket/Redis Pubsub demo

This is an example of how you might run a Rust server, using Redis as Pubsub.

The value of this is being able to run multiple Rust websocket servers, which are able to exchange messages via the Redis Pubsub so that a user connected to server A can receive messages from a user connected to server B.

_*Disclaimer: There are a lot of unwraps here, consider handling these errors if you use this code as inspo/boilerplate*_

