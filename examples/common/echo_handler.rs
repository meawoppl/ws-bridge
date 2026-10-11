//! A callback shared by the framework examples. The connection type is inferred
//! from the selected server adapter (Actix uses its local connection type).
macro_rules! echo_handler {
    () => { echo_handler!(ws_bridge::WsConnection<common::ServerMsg, common::ClientMsg>) };
    ($conn_type:ty) => {
        |conn: $conn_type| async move {
            let (mut tx, mut rx) = conn.split();
            if tx.send(common::ServerMsg::Welcome {
                message: "Hello! Send me messages and I'll echo them back.".into(),
            }).await.is_err() { return; }
            while let Some(result) = rx.recv().await {
                match result {
                    Ok(common::ClientMsg::Say { text }) => {
                        if tx.send(common::ServerMsg::Echo { payload: text }).await.is_err() { break; }
                    }
                    Ok(common::ClientMsg::Quit) | Err(ws_bridge::RecvError::Closed) => break,
                    Err(ws_bridge::RecvError::Decode(error)) => eprintln!("invalid message: {error}"),
                }
            }
            let _ = tx.close().await;
        }
    };
}
