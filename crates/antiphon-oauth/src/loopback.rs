use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

use crate::{OauthError, query};

const CALLBACK_READ_TIMEOUT: Duration = Duration::from_secs(10);

const SUCCESS_PAGE: &str = "<!doctype html><title>Antiphon\
     </title><p>Antiphon is authorised. You can close this tab.";
const FAILURE_PAGE: &str = "<!doctype html><title>Antiphon\
     </title><p>Authorisation failed. Return to Antiphon.";

pub(crate) fn wait_for_code(
    listener: &TcpListener,
    expected_state: &str,
) -> Result<String, OauthError> {
    loop {
        let (stream, _) = listener
            .accept()
            .map_err(|error| listener_failed(&error))?;
        let Some(result) = handle_callback(stream, expected_state)?
        else {
            continue;
        };
        return Ok(result);
    }
}

fn handle_callback(
    mut stream: TcpStream,
    expected_state: &str,
) -> Result<Option<String>, OauthError> {
    stream
        .set_read_timeout(Some(CALLBACK_READ_TIMEOUT))
        .map_err(|error| listener_failed(&error))?;
    let Some(target) = read_request_target(&stream) else {
        respond(&mut stream, "400 Bad Request", FAILURE_PAGE);
        return Ok(None);
    };
    let query = target.split_once('?').map(|(_, q)| q);
    let pairs = query::parse(query.unwrap_or(""));
    if let Some(error) = query::get(&pairs, "error") {
        respond(&mut stream, "200 OK", FAILURE_PAGE);
        return Err(callback_error(error, &pairs));
    }
    let Some(code) = query::get(&pairs, "code") else {
        respond(&mut stream, "404 Not Found", FAILURE_PAGE);
        return Ok(None);
    };
    if query::get(&pairs, "state") != Some(expected_state) {
        respond(&mut stream, "400 Bad Request", FAILURE_PAGE);
        return Err(OauthError::StateMismatch);
    }
    let code = code.to_string();
    respond(&mut stream, "200 OK", SUCCESS_PAGE);
    Ok(Some(code))
}

fn listener_failed(error: &std::io::Error) -> OauthError {
    OauthError::Loopback(error.to_string())
}

fn read_request_target(stream: &TcpStream) -> Option<String> {
    let mut request_line = String::new();
    BufReader::new(stream).read_line(&mut request_line).ok()?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?;
    if method != "GET" {
        return None;
    }
    parts.next().map(str::to_string)
}

fn respond(stream: &mut TcpStream, status: &str, page: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\n\
         Content-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\r\n{page}",
        page.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

fn callback_error(
    error: &str,
    pairs: &[(String, String)],
) -> OauthError {
    let detail = query::get(pairs, "error_description")
        .unwrap_or("")
        .to_string();
    if error == "access_denied" {
        return OauthError::Declined(detail);
    }
    OauthError::Protocol(format!("{error}: {detail}"))
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::{Ipv4Addr, TcpListener, TcpStream};
    use std::thread;

    use super::wait_for_code;
    use crate::OauthError;

    fn local_listener() -> (TcpListener, String) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        (listener, addr.to_string())
    }

    fn send_get(addr: &str, target: &str) {
        let mut stream = TcpStream::connect(addr).expect("connect");
        let request = format!("GET {target} HTTP/1.1\r\n\r\n");
        stream.write_all(request.as_bytes()).expect("write request");
        let mut response = String::new();
        let _ = stream.read_to_string(&mut response);
    }

    #[test]
    fn captures_code_when_state_matches() {
        let (listener, addr) = local_listener();
        let writer = thread::spawn(move || {
            send_get(&addr, "/?code=abc%2Fdef&state=expected");
        });
        let code = wait_for_code(&listener, "expected")
            .expect("code captured");
        assert_eq!(code, "abc/def");
        writer.join().expect("writer thread");
    }

    #[test]
    fn rejects_state_mismatch() {
        let (listener, addr) = local_listener();
        let writer = thread::spawn(move || {
            send_get(&addr, "/?code=abc&state=forged");
        });
        let error = wait_for_code(&listener, "expected")
            .expect_err("mismatch rejected");
        assert!(matches!(error, OauthError::StateMismatch));
        writer.join().expect("writer thread");
    }

    #[test]
    fn ignores_stray_requests_before_callback() {
        let (listener, addr) = local_listener();
        let writer = thread::spawn(move || {
            send_get(&addr, "/favicon.ico");
            send_get(&addr, "/?code=real&state=expected");
        });
        let code = wait_for_code(&listener, "expected")
            .expect("code captured");
        assert_eq!(code, "real");
        writer.join().expect("writer thread");
    }

    #[test]
    fn reports_provider_declines() {
        let (listener, addr) = local_listener();
        let writer = thread::spawn(move || {
            send_get(&addr, "/?error=access_denied&state=expected");
        });
        let error = wait_for_code(&listener, "expected")
            .expect_err("decline surfaced");
        assert!(matches!(error, OauthError::Declined(_)));
        writer.join().expect("writer thread");
    }
}
