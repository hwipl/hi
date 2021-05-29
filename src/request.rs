use async_trait::async_trait;

use futures::prelude::*;
use libp2p::core::{
    upgrade::{read_one, write_one},
    ProtocolName,
};
use libp2p::request_response::RequestResponseCodec;
use std::io;

/// Request-response protocol for the request-response behaviour
#[derive(Debug, Clone)]
pub struct HiRequestProtocol();

impl ProtocolName for HiRequestProtocol {
    fn protocol_name(&self) -> &[u8] {
        "/hi/request/0.0.1".as_bytes()
    }
}

/// Codec for the request-response behaviour
#[derive(Clone)]
pub struct HiCodec();

#[async_trait]
impl RequestResponseCodec for HiCodec {
    type Protocol = HiRequestProtocol;
    type Request = HiRequest;
    type Response = HiResponse;

    async fn read_request<T>(
        &mut self,
        _: &HiRequestProtocol,
        io: &mut T,
    ) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        read_one(io, 1024)
            .map(|res| match res {
                Err(e) => Err(io::Error::new(io::ErrorKind::InvalidData, e)),
                Ok(vec) if vec.is_empty() => Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(vec) => Ok(HiRequest::Data(vec)),
            })
            .await
    }

    async fn read_response<T>(
        &mut self,
        _: &HiRequestProtocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        read_one(io, 1024)
            .map(|res| match res {
                Err(e) => Err(io::Error::new(io::ErrorKind::InvalidData, e)),
                Ok(vec) if vec.is_empty() => Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(vec) => Ok(HiResponse(vec)),
            })
            .await
    }

    async fn write_request<T>(
        &mut self,
        _: &HiRequestProtocol,
        io: &mut T,
        request: HiRequest,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        match request {
            HiRequest::Data(data) => write_one(io, data).await,
        }
    }

    async fn write_response<T>(
        &mut self,
        _: &HiRequestProtocol,
        io: &mut T,
        HiResponse(data): HiResponse,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        write_one(io, data).await
    }
}

/// Request message
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HiRequest {
    Data(Vec<u8>),
}

/// Response message
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HiResponse(pub Vec<u8>);
