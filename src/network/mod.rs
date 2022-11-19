use serde::Serialize;
use slog::Logger;

use crate::prelude::*;

use std::io;
use std::io::Write;
use std::io::{BufRead, BufReader, BufWriter};
use std::net;
use std::os::unix::net as uxnet;
use std::path::Path;
use std::sync::Arc;

use http::Version;

/// Adapter trait which indicates if a type is able to
/// connect to some address and return a stream as a
/// result
pub trait ConnectorAdapter {
    type StreamT: io::Read + io::Write + Send + Sync;

    /// Connect to some endpoint addressed by a string path
    fn connect(addr: &str) -> io::Result<Self::StreamT>
    where
        Self: Sized;
}

/// An interface which defines a type able to send
/// arbitrary serializable data over streams
pub trait DataSenderExt<StreamT>
where
    StreamT: ConnectorAdapter,
{
    fn send_data<D>(&self, data: D) -> UResult
    where
        D: Serialize;
}

/// An interface for handling dispatched interprocess
/// commands received from another bots in *qcproto*
/// protocol
pub trait CommandHandler: Send + Sync {
    /// A request for forwarding a message to
    /// another arbitrary platform
    fn forward_message(&self, _msg: Command) -> UResult;
}

/// An interface for handling established connections,
/// or any other readable/writable stream of data
pub trait StreamHandler<T>
where
    T: io::Read + io::Write,
    Self: Send + Sync,
{
    fn handle_stream(&self, stream: T) -> UResult;
}

/// Convenient type for wrapping an Arc to a StreamHandler type
/// which takes the listener's bound stream type
pub type StreamHandlerArc<ListenerT> =
    Arc<dyn StreamHandler<<ListenerT as ListenerAdapter>::StreamT>>;

/// Convenient type for wrapping a bare reference to a StreamHandler type
/// which takes the listener's bound stream type
pub type StreamHandlerRef<'a, ListenerT> =
    &'a dyn StreamHandler<<ListenerT as ListenerAdapter>::StreamT>;

/// A wrapper trait which allows to configure any type
/// of listener to be used with a StreamListener
pub trait ListenerAdapter: Send + Sync {
    type StreamT: io::Read + io::Write + Send + Sync;
    type SockAddrT;

    fn accept(&self) -> io::Result<(Self::StreamT, Self::SockAddrT)>;
}

impl ConnectorAdapter for net::TcpStream {
    type StreamT = net::TcpStream;

    fn connect(path: &str) -> io::Result<Self::StreamT>
    where
        Self: Sized,
    {
        net::TcpStream::connect(path)
    }
}

impl ConnectorAdapter for uxnet::UnixStream {
    type StreamT = uxnet::UnixStream;

    fn connect(addr: &str) -> io::Result<Self::StreamT>
    where
        Self: Sized,
    {
        uxnet::UnixStream::connect(addr)
    }
}

impl ListenerAdapter for net::TcpListener {
    type StreamT = net::TcpStream;
    type SockAddrT = net::SocketAddr;

    fn accept(&self) -> io::Result<(Self::StreamT, Self::SockAddrT)> {
        self.accept()
    }
}

impl ListenerAdapter for uxnet::UnixListener {
    type StreamT = uxnet::UnixStream;
    type SockAddrT = uxnet::SocketAddr;

    fn accept(&self) -> io::Result<(Self::StreamT, Self::SockAddrT)> {
        self.accept()
    }
}

/// An interface which allows any type to listen
/// on a configured listener type, implementing a
/// listener adapter
pub trait StreamListenerExt<ListenerT>
where
    ListenerT: ListenerAdapter,
    Self: Send + Sync,
{
    fn request_stop(&self);

    fn is_stopped(&self) -> bool;

    fn listen(&self) -> UResult;
}

/// Convenience function allowing to transform a u8
/// into its equivalent enumerated version of the
/// http crate
fn parse_http_version(v: u8) -> UResult<http::Version> {
    match v {
        0 => Ok(http::Version::HTTP_09),
        1 => Ok(http::Version::HTTP_10),
        2 => Ok(http::Version::HTTP_2),
        3 => Ok(http::Version::HTTP_3),
        _ => Err("Wrong HTTP version".into()),
    }
}

/// Read a stream expecting a valid unencrypted HTTP request
pub fn read_http_request<T>(stream: &mut T) -> UResult<http::Request<String>>
where
    T: std::io::Read,
{
    let mut headers = [httparse::EMPTY_HEADER; 16];
    let mut reader = BufReader::new(stream);
    let buffer = reader.fill_buf()?.to_vec();
    let mut request_infos = httparse::Request::new(&mut headers);
    let parse_result = request_infos.parse(&buffer)?;
    if parse_result.is_partial() {
        return Err("Incoming HTTP request is not complete".into());
    }
    let parse_result = parse_result.unwrap();
    let mut request = http::Request::builder()
        .method(request_infos.method.unwrap())
        .uri(request_infos.path.unwrap())
        .version(parse_http_version(request_infos.version.unwrap())?);
    for header in headers {
        if header != httparse::EMPTY_HEADER {
            request = request.header(header.name, header.value);
        }
    }
    let buffer: Vec<u8> = buffer.into_iter().skip(parse_result).collect();
    let request = request.body(String::from_utf8(buffer)?)?;
    Ok(request)
}

/// Write a valid unencrypted HTTP response into a given writable stream
pub fn write_http_response<T>(stream: &mut T, response: http::Response<&str>) -> UResult
where
    T: std::io::Write,
{
    let (parts, body) = response.into_parts();
    let version = match parts.version {
        Version::HTTP_09 => "HTTP/0.9",
        Version::HTTP_10 => "HTTP/1.0",
        Version::HTTP_11 => "HTTP/1.1",
        Version::HTTP_2 => "HTTP/2.0",
        Version::HTTP_3 => "HTTP/3.0",
        _ => return Err("Impossible version enum error".into()),
    };
    let buffer = Vec::new();
    let mut bufwriter = BufWriter::new(buffer);
    write!(
        bufwriter,
        "{} {} {}\r\n",
        version,
        parts.status.as_str(),
        parts.status.canonical_reason().unwrap()
    )?;
    for (key, value) in parts.headers {
        let key = key.unwrap();
        write!(
            bufwriter,
            "{}: {}\r\n",
            key,
            String::from_utf8(value.as_bytes().to_vec())?
        )?;
    }
    write!(bufwriter, "\r\n{}", body)?;
    let data = String::from_utf8(bufwriter.buffer().to_vec())?;
    write!(stream, "{}", &data)?;
    Ok(())
}
