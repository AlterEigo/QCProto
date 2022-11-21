use std::marker::PhantomData;

use crate::prelude::*;
use crate::types::PROTOCOL_VERSION;
use slog::Logger;
use std::io;
use std::io::{Read, Write};

use std::net::{TcpListener, TcpStream};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::ScopedJoinHandle;

use slog::{crit, debug, error, info, o, warn};

#[derive(Debug)]
pub struct DataSender<StreamT>
where
    StreamT: ConnectorAdapter,
{
    destination: String,
    stream_type: PhantomData<StreamT>,
}

impl<StreamT> DataSender<StreamT>
where
    StreamT: ConnectorAdapter,
{
    pub fn new(destination: String) -> Self {
        Self {
            destination,
            stream_type: PhantomData,
        }
    }
}

impl<StreamT> DataSenderExt<StreamT> for DataSender<StreamT>
where
    StreamT: ConnectorAdapter,
{
    fn send_data<D>(&self, data: D) -> UResult
    where
        D: serde::Serialize,
    {
        let mut connection = StreamT::connect(&self.destination)?;
        let payload = serde_json::to_string(&data)?;
        Ok(write!(connection, "{}", payload)?)
    }
}

#[derive(Debug)]
pub struct CommandSender {
    sender: DataSender<UnixStream>,
}

impl CommandSender {
    pub fn new(destination: String) -> Self {
        Self {
            sender: DataSender::new(destination),
        }
    }

    pub fn send(&self, cmd: Command) -> UResult {
        self.sender.send_data(cmd)
    }
}

/// Application's local command server implementation
/// allowing to communicate with other bots using qcproto
/// running on the local machine via unix sockets
pub struct CommandServer {
    stream_listener: Arc<dyn StreamListenerExt<UnixListener>>,
}

#[derive(Default)]
pub struct CommandServerBuilder {
    stream_handler: Option<Arc<dyn StreamHandler<UnixStream>>>,
    stream_listener: Option<Arc<dyn StreamListenerExt<UnixListener>>>,
    logger: Option<Logger>,
    bind_addr: Option<String>,
}

impl CommandServer {
    /// Instantiate a new command server
    pub fn new() -> CommandServerBuilder {
        Default::default()
    }
}

impl CommandServerBuilder {
    /// Set the address for server in the format accepted by
    /// the standard `std::os::unix::UnixListener` type ('UNIX_FILEPATH')
    pub fn server_addr(self, addr: &str) -> Self {
        Self {
            bind_addr: Some(String::from(addr)),
            ..self
        }
    }

    /// Set the logger for the server and all of the initialized
    /// default modules
    pub fn logger(self, logger: Logger) -> Self {
        Self {
            logger: Some(logger.new(o!("module" => "CommandServer"))),
            ..self
        }
    }

    /// Set a handler for all established connections via unix sockets
    pub fn stream_handler(self, handler: Arc<dyn StreamHandler<UnixStream>>) -> Self {
        Self {
            stream_handler: Some(handler),
            ..self
        }
    }

    /// Set a custom unix socket listener
    pub fn stream_listener<ListenerT>(
        self,
        listener: Arc<dyn StreamListenerExt<UnixListener>>,
    ) -> Self {
        Self {
            stream_listener: Some(listener),
            ..self
        }
    }

    /// Finalize the update server construction.
    ///
    /// Construction of the server fails in the following scenarios:
    /// - A logger is not provided
    /// - Both server address and custom stream listener are provided, or neither of them
    /// - A custom stream handler is provided with the custom stream listener
    /// - If an error occurs while initializing one of the subcomponents
    pub fn build(self) -> UResult<CommandServer> {
        let flags = (
            self.logger.is_some(),
            self.bind_addr.is_some(),
            self.stream_listener.is_some(),
            self.stream_handler.is_some(),
        );
        match flags {
            (false, _, _, _) => panic!("Did not provide a logger for the command server"),
            (_, true, true, _) | (_, false, false, _) => {
                panic!("You have to provide either an address to bind to, or a configured listener")
            }
            (_, _, true, true) => {
                panic!("A custom stream handler won't be used if you also provide a listener")
            }
            _ => (),
        };

        let stream_handler = self.stream_handler.unwrap();
        let stream_listener = self.stream_listener.unwrap_or(Arc::new(
            StreamListener::<UnixListener>::new()
                .listener(UnixListener::bind(self.bind_addr.unwrap())?)
                .stream_handler(stream_handler)
                .logger(self.logger.unwrap())
                .build(),
        ));
        let srv = CommandServer { stream_listener };
        Ok(srv)
    }
}

impl StreamListenerExt<UnixListener> for CommandServer {
    fn listen(&self) -> UResult {
        self.stream_listener.listen()
    }

    fn request_stop(&self) {
        self.stream_listener.request_stop()
    }

    fn is_stopped(&self) -> bool {
        self.stream_listener.is_stopped()
    }
}

/// Default implementation of a stream listener for
/// any type which implements the ListenerAdapter
/// trait
pub struct StreamListener<ListenerT>
where
    ListenerT: ListenerAdapter,
{
    logger: Logger,
    listener: ListenerT,
    stream_handler: Option<StreamHandlerArc<ListenerT>>,
    stop_requested: AtomicBool,
}

/// A builder type for instantiating the default
/// stream listener
pub struct StreamListenerBuilder<T>
where
    T: ListenerAdapter,
{
    listener: Option<T>,
    logger: Option<Logger>,
    handler: Option<StreamHandlerArc<T>>,
}

impl<T> Default for StreamListenerBuilder<T>
where
    T: ListenerAdapter,
{
    fn default() -> Self {
        StreamListenerBuilder {
            listener: None,
            logger: None,
            handler: None,
        }
    }
}

impl<T> StreamListenerBuilder<T>
where
    T: ListenerAdapter,
{
    /// Set the listener type
    pub fn listener(self, new_listener: T) -> Self {
        Self {
            listener: Some(new_listener),
            ..self
        }
    }

    /// Set the listener's logger
    pub fn logger(self, new_logger: Logger) -> Self {
        Self {
            logger: Some(new_logger),
            ..self
        }
    }

    /// Set an entity which will handle all established connections
    pub fn stream_handler(self, handler: StreamHandlerArc<T>) -> Self {
        Self {
            handler: Some(handler),
            ..self
        }
    }

    /// Finalize the instantiation of a stream listener
    pub fn build(self) -> StreamListener<T> {
        StreamListener {
            logger: self
                .logger
                .expect("Did not provide a logger for StreamListenerBuilder"),
            listener: self
                .listener
                .expect("Did not provide a listener type for StreamListenerBuilder"),
            stop_requested: AtomicBool::new(false),
            stream_handler: self.handler,
        }
    }
}

impl<ListenerT> StreamListener<ListenerT>
where
    ListenerT: ListenerAdapter,
{
    /// Instantiate a new default stream listener
    pub fn new() -> StreamListenerBuilder<ListenerT> {
        StreamListenerBuilder::<ListenerT>::default()
    }

    /// Change the stream handler on the fly
    pub fn set_handler<'b>(&'b mut self, handler: StreamHandlerArc<ListenerT>) -> &'b mut Self {
        self.stream_handler = Some(handler);
        self
    }
}

impl<ListenerT> StreamListenerExt<ListenerT> for StreamListener<ListenerT>
where
    ListenerT: ListenerAdapter,
{
    fn request_stop(&self) {
        self.stop_requested.store(false, Ordering::Relaxed)
    }

    fn is_stopped(&self) -> bool {
        self.stop_requested.load(Ordering::Relaxed)
    }

    /// Engage the the loop for processing new connections in the
    /// current thread
    fn listen(&self) -> UResult {
        // A scope for each new spawned thread. All threads
        // spawned into a scope are guaranteed to be destroyed
        // before the function returns
        std::thread::scope(|scope| -> UResult {
            // New container for all worker threads
            let mut workers: Vec<ScopedJoinHandle<UResult>> = Vec::new();

            // Iterating through the connection queue and
            // spawning a new handler thread for each new
            // connection
            loop {
                let result = self.listener.accept();
                // let (stream, _) = self.listener.accept()?;
                debug!(self.logger, "Handling incoming request");

                // Handling connection errors before processing
                if let Err(err) = result {
                    match err.kind() {
                        io::ErrorKind::WouldBlock => continue,
                        _ => {
                            error!(self.logger, "TCP stream error"; "reason" => err.to_string());
                            return Err(err.into());
                        }
                    };
                }
                let (stream, _) = result.unwrap();

                // If a connection handler is available, we spawn
                // a new thread and delegating the connection processing
                // to this external stream handler
                if let Some(ref handler) = self.stream_handler {
                    let logger = self.logger.clone();
                    let handler = handler.clone();
                    let worker = scope.spawn(move || {
                        if let Err(why) = handler.handle_stream(stream) {
                            error!(logger, "TCP stream handling error"; "error" => format!("{:#?}", why));
                        }
                        Ok(())
                    });
                    workers.push(worker);
                }

                // Checking if the client requested server stop
                if self.is_stopped() {
                    break;
                }
            }

            // Joining all threads manually and handling
            // errors before qutting the scope
            for w in workers {
                if let Err(why) = w.join() {
                    error!(self.logger, "Error while joining the worker thread"; "reason" => format!("{:#?}", why));
                }
            }
            Ok(())
        })
    }
}

/// Default implementation of a qcproto command handler
pub struct DefaultCommandHandler {
    logger: Logger,
}

impl DefaultCommandHandler {
    /// Instantiate a new default command handler
    pub fn new(logger: Logger) -> Self {
        Self { logger }
    }
}

impl CommandHandler for DefaultCommandHandler {
    fn forward_message(&self, _msg: Command) -> UResult {
        todo!()
    }
}

/// Default implementation of a unix stream handler
///
/// The default implementation expects that the data
/// being transmitted over the stream is done according
/// to the qcproto protocol
pub struct DefaultUnixStreamHandler {
    dispatcher: Arc<dyn Dispatcher<Command>>,
    logger: Logger,
}

impl DefaultUnixStreamHandler {
    /// Instantiate a new default unix stream handler
    pub fn new(
        dispatcher: Arc<dyn Dispatcher<Command>>,
        logger: Logger,
    ) -> DefaultUnixStreamHandler {
        DefaultUnixStreamHandler {
            dispatcher,
            logger: logger.new(o!("module" => "DefaultUnixStreamHandler")),
        }
    }
}

impl StreamHandler<UnixStream> for DefaultUnixStreamHandler {
    fn handle_stream(&self, mut stream: UnixStream) -> UResult {
        let response = serde_json::to_string(&TransmissionResult::Received)?;
        let mut buffer = String::new();
        stream.read_to_string(&mut buffer);
        let command = serde_json::from_str::<Command>(&buffer);
        if let Err(_) = command {
            write!(
                stream,
                "{}",
                serde_json::to_string(&TransmissionResult::BadSyntax)?
            );
        }
        let command = command.unwrap();
        let used_protocol = PROTOCOL_VERSION;
        if command.protocol_version != used_protocol {
            let response = serde_json::to_string(&TransmissionResult::MismatchedVersions)?;
            write!(stream, "{}", response);
            return Err(format!("{:?}", TransmissionResult::MismatchedVersions).into());
        }
        write!(stream, "{}", response);
        self.dispatcher.dispatch(command)
    }
}

/// Default implementation of an interprocess
/// command dispatcher
pub struct DefaultCommandDispatcher {
    handler: Arc<dyn CommandHandler>,
    logger: Logger,
}

impl DefaultCommandDispatcher {
    /// Instantiate a new default Telegram update dispatcher
    pub fn new(handler: Arc<dyn CommandHandler>, logger: Logger) -> Self {
        Self {
            handler: handler.clone(),
            logger: logger.new(o!("module" => "DefaultCommandDispatcher")),
        }
    }
}

impl Dispatcher<Command> for DefaultCommandDispatcher {
    fn dispatch(&self, data: Command) -> UResult {
        info!(self.logger, "Incoming command: {:#?}", data);
        let used_protocol = PROTOCOL_VERSION;
        if used_protocol != data.protocol_version {
            error!(
                self.logger,
                "Mismatched protocol versions, expected: {}; received: {}",
                used_protocol,
                data.protocol_version
            );
            return Err(format!("{:?}", TransmissionResult::MismatchedVersions).into());
        }

        match &data.kind {
            CommandKind::ForwardMessage { .. } => self.handler.forward_message(data),
            _ => {
                warn!(self.logger, "Unhandled command kind: {:#?}", data);
                Ok(())
            }
        }
    }
}
