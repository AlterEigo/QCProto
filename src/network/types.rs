use std::marker::PhantomData;
use std::os::unix::net::UnixStream;

use crate::prelude::*;
use std::io;
use std::io::Write;

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
