use std::ops::ControlFlow;
use std::os::unix::net::UnixStream;
use std::thread;

use antiphon_ipc::{
    Event, IpcClient, IpcServer, OpId, OpKind, Operation, Request,
    Response, read_frame, write_frame,
};

fn answer(stream: &mut UnixStream) -> ControlFlow<()> {
    loop {
        let Ok(request) = read_frame::<Request, _>(stream) else {
            return ControlFlow::Continue(());
        };
        match request {
            Request::Ping => {
                write_frame(stream, &Response::Pong).unwrap();
            }
            Request::EnqueueOp(_) => {
                write_frame(stream, &Response::Ack).unwrap();
            }
            Request::Subscribe => {
                write_frame(stream, &Response::Ack).unwrap();
                write_frame(stream, &Event::SyncStarted).unwrap();
                write_frame(stream, &Event::OpApplied(OpId(41)))
                    .unwrap();
                return ControlFlow::Break(());
            }
            _ => {
                let refusal =
                    Response::Error("not in this test".into());
                write_frame(stream, &refusal).unwrap();
            }
        }
    }
}

#[test]
fn a_client_and_daemon_talk_over_a_real_socket() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("antiphon").join("antiphond.sock");
    let server = IpcServer::bind(&path).unwrap();
    let daemon = thread::spawn(move || {
        server.serve(|mut stream| answer(&mut stream)).unwrap();
    });

    let mut client = IpcClient::connect(&path).unwrap();
    assert_eq!(client.request(&Request::Ping).unwrap(), Response::Pong);

    let operation = Operation {
        op_id: OpId(41),
        account: "example".into(),
        message_id: "<one@example.com>".into(),
        kind: OpKind::Flag {
            add: vec!["flagged".into()],
            remove: vec!["unread".into()],
        },
    };
    assert_eq!(
        client.request(&Request::EnqueueOp(operation)).unwrap(),
        Response::Ack
    );

    let mut events = client.subscribe().unwrap();
    assert_eq!(events.next().unwrap().unwrap(), Event::SyncStarted);
    assert_eq!(
        events.next().unwrap().unwrap(),
        Event::OpApplied(OpId(41))
    );
    daemon.join().unwrap();
    assert!(events.next().is_none());
}
