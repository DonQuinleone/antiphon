use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::thread::{self, JoinHandle};

pub(crate) struct StubResponse {
    status: &'static str,
    body: String,
}

pub(crate) fn ok(body: &str) -> StubResponse {
    StubResponse {
        status: "200 OK",
        body: body.to_string(),
    }
}

pub(crate) fn bad_request(body: &str) -> StubResponse {
    StubResponse {
        status: "400 Bad Request",
        body: body.to_string(),
    }
}

pub(crate) struct Stub {
    pub base_url: String,
    handle: JoinHandle<Vec<String>>,
}

impl Stub {
    pub fn serve(responses: Vec<StubResponse>) -> Stub {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .expect("bind stub listener");
        let base_url = format!(
            "http://{}",
            listener.local_addr().expect("stub addr")
        );
        let handle = thread::spawn(move || {
            responses
                .into_iter()
                .map(|response| answer(&listener, response))
                .collect()
        });
        Stub { base_url, handle }
    }

    pub fn finish(self) -> Vec<String> {
        self.handle.join().expect("stub thread")
    }
}

fn answer(listener: &TcpListener, response: StubResponse) -> String {
    let (mut stream, _) = listener.accept().expect("stub accept");
    let request = read_request(&mut stream);
    let reply = format!(
        "HTTP/1.1 {}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\r\n{}",
        response.status,
        response.body.len(),
        response.body
    );
    stream.write_all(reply.as_bytes()).expect("stub write");
    request
}

fn read_request(stream: &mut TcpStream) -> String {
    let mut reader = BufReader::new(stream);
    let mut request = String::new();
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).expect("stub read");
        if let Some(length) = header_value(&line, "content-length") {
            content_length = length.parse().expect("content length");
        }
        let done = line == "\r\n" || line.is_empty();
        request.push_str(&line);
        if done {
            break;
        }
    }
    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body).expect("stub body");
    request.push_str(&String::from_utf8_lossy(&body));
    request
}

fn header_value<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let (key, value) = line.split_once(':')?;
    if !key.eq_ignore_ascii_case(name) {
        return None;
    }
    Some(value.trim())
}
