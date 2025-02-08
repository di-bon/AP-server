# AP-server
This repository contains the server code of the group "The Null Pointer Patrol" for the Advanced Programming course held at the University of Trento in the academic year 2024-2025

## Description
The ```DibServer``` can be used to instantiate either a 
```CommunicationServer``` or a ```ContentServer```.

The ```CommunicationServer``` is used to forward messages 
between two clients. In order to send/receive messages,
a client must register to the server.

The ```ContentServer``` is used to give clients access to text and 
media resources. Media resources are referenced inside text using 
tow curly braces. An example of media referencing is: 
```
Lorem ipsum {{ image.png }} dolor sit amet
```

## Usage
To use the Server, add
```toml
ap_server = { git = "https://github.com/di-bon/AP-server" }
```
to your Cargo.toml file.

Then, import it in your project files using
```rust
use ap_server::{DibServer, DibServerTrait};
```

To create a new DibServer, use the constructor 
```DibServer::new_communication_server()``` 
or ```DibServer::new_content_server()```.

To make the DibServer work, call ```DibServerTrait::run()```.

## Panics
See the documentation for each function.
