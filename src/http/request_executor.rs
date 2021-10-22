use crate::abi::commands::AbiCommand;
use crate::abi::dispatcher::{AsyncResult, AsyncType};
use http::HeaderMap;
use hyper::client::connect::HttpConnector;
use hyper_tls::HttpsConnector;
use log::debug;
use std::convert::TryFrom;
use tokio::sync::mpsc::{Sender, UnboundedReceiver};

use crate::abi::rust_v1alpha1::HttpResponse;

pub async fn start_request_executor(
    mut rx: UnboundedReceiver<AbiCommand<http::Request<Vec<u8>>>>,
    otx: Sender<AsyncResult>,
    ocluster_url: http::Uri,
    ohttp_client: hyper::Client<HttpsConnector<HttpConnector>, hyper::Body>,
) -> anyhow::Result<()> {
    while let Some(mut http_command) = rx.recv().await {
        let cluster_url = ocluster_url.clone();
        let http_client = ohttp_client.clone();
        let tx = otx.clone();
        tokio::spawn(async move {
            // Patch the request URI
            *http_command.value.uri_mut() = http::Uri::try_from(super::generate_url(
                &cluster_url,
                http_command.value.uri().path_and_query().unwrap(),
            ))
            .expect("Cannot build the final uri");

            debug!(
                "Received request command from '{}' with id {}: {} {:?}",
                &http_command.controller_name,
                &http_command.async_request_id,
                http_command.value.method().as_str(),
                http_command.value.uri()
            );

            // Execute the request
            let response = http_client
                .clone()
                .request(http_command.value.map(hyper::Body::from))
                .await
                .expect("Successful response");

            // Serialize the response
            let status_code = response.status();
            let mut headers = HeaderMap::with_capacity(response.headers().len());
            for (k, v) in response.headers().iter() {
                headers.append(k, v.clone());
            }

            let body = hyper::body::to_bytes(response.into_body())
                .await
                .expect("error while receiving");

            tx.clone()
                .send(AsyncResult {
                    controller_name: http_command.controller_name.clone(),
                    async_request_id: http_command.async_request_id,
                    async_type: AsyncType::Future,
                    value: Some(
                        bincode::serialize(&HttpResponse {
                            status_code,
                            headers,
                            body: body.to_vec(),
                        })
                        .expect("Error while serializing"),
                    ),
                })
                .await
                .expect("Send error");
        });
    }

    Ok(())
}