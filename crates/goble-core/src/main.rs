use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;

fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    println!("Listening on http://{}", addr);

    for stream in listener.incoming() {
        let mut stream = stream?;
        let mut reader = BufReader::new(&mut stream);
        let mut request_line = String::new();
        reader.read_line(&mut request_line)?;

        let parts: Vec<&str> = request_line.trim().split_whitespace().collect();

        let (method, path) = match (parts.first(), parts.get(1)) {
            (Some(m), Some(p)) => (*m, *p),
            _ => continue,
        };

        if method == "GET" && path == "/" {
            let response = "HTTP/1.1 200 OK\r\nContent-Length: 17\r\nContent-Type: text/plain\r\n\r\nHello from Goble";
            stream.write_all(response.as_bytes())?;
        } else {
            let body = "Not Found";
            let response = format!(
                "HTTP/1.1 404 Not Found\r\nContent-Length: {}\r\nContent-Type: text/plain\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes())?;
        }
        stream.flush()?;
    }

    Ok(())
}
