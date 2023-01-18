use std::borrow::BorrowMut;
use std::future::Future;
use std::num::NonZeroUsize;
use std::ops::ControlFlow;
use std::sync::Arc;

use futures::FutureExt;
use http::{Method, StatusCode};
use http::header::CONTENT_TYPE;
use schemars::JsonSchema;
use serde::Deserialize;
use tower::{BoxError, ServiceBuilder, ServiceExt};

use apollo_router::{graphql, register_plugin};
use apollo_router::layers::ServiceBuilderExt;
use apollo_router::plugin::Plugin;
use apollo_router::plugin::PluginInit;
use apollo_router::services::{router, supergraph, TryIntoHeaderName, TryIntoHeaderValue};
use apollo_router::services::execution;
use apollo_router::services::router::{Body, Response};
use apollo_router::services::subgraph;
use graphql::Error;

#[derive(Clone)]
pub struct APQLayer {
    cache: String,
}

impl APQLayer {
    async fn request(
        &self,
        request: router::Request,
    ) -> Result<router::Request, router::Response> {
        // snip todo - actual apq logic
        return Ok(request)
    }
}

struct Apq {
    #[allow(dead_code)]
    configuration: Conf,
    apq_layer: APQLayer,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
struct Conf {
    // Put your plugin configuration here. It will automatically be deserialized from JSON.
    // Always put some sort of config here, even if it is just a bool to say that the plugin is enabled,
    // otherwise the yaml to enable the plugin will be confusing.
    enabled: bool,
}

#[async_trait::async_trait]
impl Plugin for Apq {
    type Config = Conf;

    async fn new(init: PluginInit<Self::Config>) -> Result<Self, BoxError> {
        Ok(Apq { configuration: init.config, apq_layer: APQLayer { cache: "Some caching functionality".to_string()} })
    }

    fn router_service(&self, service: router::BoxService) -> router::BoxService {
        // clone apq for use in async checkpoint, tried using Arc<APQLayer> as well, also tried by reference
        let apq = self.apq_layer.clone();

        let asy = move |req: router::Request| {
            // let apq_result = apq.request(req);
            return async {
                let apq_result = apq.request(req).await; // this doesn't work
                match apq_result {
                    Ok(request) => {
                        // either query has been replaced or query and hash were registered
                        // we can now continue the chain
                        Ok(ControlFlow::Continue(request))
                    },
                    Err(router_response) => {
                        Ok(ControlFlow::Break(router_response))
                    }
                }
            }.boxed();
        };

        ServiceBuilder::new()
            .checkpoint_async(asy)
            .buffered()
            .service(service)
            .boxed()
    }
}

// This macro allows us to use it in our plugin registry!
// register_plugin takes a group name, and a plugin name.
register_plugin!("bfgrouter", "apq", Apq);

#[cfg(test)]
mod tests {
    use tower::BoxError;
    use tower::ServiceExt;

    use apollo_router::services::{router, supergraph};
    use apollo_router::services::supergraph::Response;
    use apollo_router::TestHarness;
    use bytes::Bytes;
    use once_cell::sync::Lazy;

    #[tokio::test]
    async fn basic_test() -> Result<(), BoxError> {
        let test_harness = TestHarness::builder()
            .configuration_json(serde_json::json!({
                "plugins": {
                    "bfgrouter.apq": {
                        "message" : "Starting my plugin",
                        "enabled" : true
                    }
                }
            }))
            .unwrap()
            .build_supergraph()
            .await
            .unwrap();

        let request = supergraph::Request::canned_builder().build().unwrap();
        let mut streamed_response = test_harness.oneshot(request).await?;

        let first_response = streamed_response
            .next_response()
            .await
            .expect("couldn't get primary response");

        assert!(first_response.data.is_some());

        println!("first response: {:?}", first_response);
        let next = streamed_response.next_response().await;
        println!("next response: {:?}", next);

        // You could keep calling .next_response() until it yields None if you're expexting more parts.
        assert!(next.is_none());
        Ok(())
    }



    static EXPECTED_RESPONSE: Lazy<Bytes> = Lazy::new(|| {
        Bytes::from_static(r#"{"data":{"topProducts":[{"upc":"1","name":"Table","reviews":[{"id":"1","product":{"name":"Table"},"author":{"id":"1","name":"Ada Lovelace"}},{"id":"4","product":{"name":"Table"},"author":{"id":"2","name":"Alan Turing"}}]},{"upc":"2","name":"Couch","reviews":[{"id":"2","product":{"name":"Couch"},"author":{"id":"1","name":"Ada Lovelace"}}]}]}}"#.as_bytes())
    });

    #[tokio::test]
    async fn basic_router_test() -> Result<(), BoxError> {
        let test_harness = TestHarness::builder()
            .configuration_json(serde_json::json!({
                "plugins": {
                    "bfgrouter.apq": {
                        "message" : "Starting my plugin",
                        "enabled" : true
                    }
                }
            }))
            .unwrap()
            .build_router()
            .await
            .unwrap();

        let request = supergraph::Request::canned_builder().build().unwrap();
        let router_request = router::Request::try_from(request).unwrap();
        let mut streamed_response: router::Response = test_harness.oneshot(router_request).await?;

        let first_response = streamed_response
            .next_response()
            .await
            .expect("couldn't get primary response")
            .unwrap();

        assert_eq!(*EXPECTED_RESPONSE, first_response);
        Ok(())
    }
}