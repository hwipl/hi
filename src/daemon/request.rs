use async_std::io;
use async_trait::async_trait;
use futures::prelude::*;
use libp2p::core::{
    upgrade::{read_one, write_one},
    ProtocolName,
};
use libp2p::request_response::RequestResponseCodec;
use minicbor::{Decode, Encode};

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
                Ok(vec) => {
                    minicbor::decode(&vec).map_err(|e| io::Error::new(io::ErrorKind::Other, e))
                }
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
                Ok(vec) => {
                    minicbor::decode(&vec).map_err(|e| io::Error::new(io::ErrorKind::Other, e))
                }
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
        let mut buffer = Vec::new();
        if let Err(e) = minicbor::encode(request, &mut buffer) {
            error!("error encoding request message: {}", e);
            return Err(io::Error::new(io::ErrorKind::Other, e));
        }
        write_one(io, buffer).await
    }

    async fn write_response<T>(
        &mut self,
        _: &HiRequestProtocol,
        io: &mut T,
        response: HiResponse,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let mut buffer = Vec::new();
        if let Err(e) = minicbor::encode(response, &mut buffer) {
            error!("error encoding response message: {}", e);
            return Err(io::Error::new(io::ErrorKind::Other, e));
        }
        write_one(io, buffer).await
    }
}

/// Request message
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub enum HiRequest {
    #[n(0)]
    Data(#[n(0)] Vec<u8>),
    #[n(1)]
    FileMessage(#[n(0)] u16, #[n(1)] u16, #[n(2)] Vec<u8>),
    #[n(2)]
    Message(#[n(0)] u16, #[n(1)] u16, #[n(2)] u16, #[n(3)] Vec<u8>),
}

/// Response message
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub enum HiResponse {
    #[n(0)]
    Ok,
    #[n(1)]
    Error(#[n(0)] String),
    #[n(2)]
    Data(#[n(0)] Vec<u8>),
}
