use async_std::fs::{self, File};
use async_std::io;
use async_trait::async_trait;
use futures::prelude::*;
use libp2p::core::{
    upgrade::{read_one, write_one, write_with_len_prefix},
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
        let response = read_one(io, 1024)
            .map(|res| match res {
                Err(e) => Err(io::Error::new(io::ErrorKind::InvalidData, e)),
                Ok(vec) if vec.is_empty() => Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(vec) => {
                    minicbor::decode(&vec).map_err(|e| io::Error::new(io::ErrorKind::Other, e))
                }
            })
            .await;

        match &response {
            Ok(HiResponse::DownloadFile(file, size)) => {
                println!("got download file response: {} ({})", file, size);
                download_file(file.clone(), *size, io).await;
            }
            _ => (),
        }
        response
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
            eprintln!("error encoding request message: {}", e);
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
        // special response handling
        match response {
            HiResponse::DownloadFile(file, ..) => {
                return upload_file(file, io).await;
            }
            _ => (),
        }

        // send message
        let mut buffer = Vec::new();
        if let Err(e) = minicbor::encode(response, &mut buffer) {
            eprintln!("error encoding response message: {}", e);
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
    ChatMessage(#[n(0)] String),
    #[n(2)]
    GetFiles,
    #[n(3)]
    DownloadFile(#[n(0)] String),
    #[n(4)]
    FileMessage(#[n(0)] Vec<u8>),
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
    #[n(3)]
    FileList(#[n(0)] Vec<(String, u64)>),
    #[n(4)]
    DownloadFile(#[n(0)] String, #[n(1)] u64),
}

/// download file coming from other peer
async fn download_file<T>(file: String, size: u64, io: &mut T)
where
    T: AsyncRead + Unpin + Send,
{
    if let Ok(mut out) = File::create(format!("temp-{}", file)).await {
        if let Err(e) = io::copy(&mut io.take(size), &mut out).await {
            eprintln!("error downloading file {}: {}", file, e);
        }
    }
}

/// upload file to other peer
async fn upload_file<T>(file: String, io: &mut T) -> io::Result<()>
where
    T: AsyncWrite + Unpin + Send,
{
    // check actual file size
    let size = fs::metadata(&file).await?.len();

    // send response with actual file size
    let response = HiResponse::DownloadFile(file.clone(), size);
    let mut buffer = Vec::new();
    if let Err(e) = minicbor::encode(response, &mut buffer) {
        eprintln!("error encoding response message: {}", e);
        return Err(io::Error::new(io::ErrorKind::Other, e));
    }
    write_with_len_prefix(io, buffer).await?;

    // upload file
    let mut input = File::open(file.clone()).await?;
    if let Err(e) = io::copy(&mut input, io).await {
        eprintln!("error uploading file {}: {}", file, e);
        return Err(io::Error::new(io::ErrorKind::Other, e));
    }

    // close connection
    io.close().await
}
