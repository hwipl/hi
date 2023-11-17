use async_std::io;
use async_trait::async_trait;
use futures::prelude::*;
use libp2p::request_response;
use minicbor::{Decode, Encode};

/// Request-response protocol for the request-response behaviour
#[derive(Debug, Clone)]
pub struct HiRequestProtocol();

impl AsRef<str> for HiRequestProtocol {
    fn as_ref(&self) -> &str {
        "/hi/request/0.0.1"
    }
}

/// Codec for the request-response behaviour
#[derive(Clone, Default)]
pub struct HiCodec();

#[async_trait]
impl request_response::Codec for HiCodec {
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
        let mut vec = Vec::new();
        io.take(1024).read_to_end(&mut vec).await?;
        minicbor::decode(&vec).map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    async fn read_response<T>(
        &mut self,
        _: &HiRequestProtocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut vec = Vec::new();
        io.take(1024).read_to_end(&mut vec).await?;
        minicbor::decode(&vec).map_err(|e| io::Error::new(io::ErrorKind::Other, e))
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
        io.write_all(buffer.as_ref()).await
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
        io.write_all(buffer.as_ref()).await
    }
}

/// Request message
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub enum HiRequest {
    #[n(0)]
    Message(
        #[n(0)] u16,
        #[n(1)] u16,
        #[n(2)] u16,
        #[n(3)]
        #[cbor(with = "minicbor::bytes")]
        Vec<u8>,
    ),
}

/// Response message
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub enum HiResponse {
    #[n(0)]
    Ok,
    #[n(1)]
    Error(#[n(0)] String),
}
