//! Serial fixed-corpus correctness execution over Eggfetch.

use eggbench_core::{
    HttpCaseBodyV1, HttpCorpusCaseDisposition, HttpCorpusCaseResultV1, HttpCorpusCheckResultV1,
    load_http_security_corpus,
};
use eggbench_runner::{
    CorrectnessContext, CorrectnessDisposition, CorrectnessExecutor, CorrectnessOutput,
    FailureCategory,
};
use sha2::{Digest, Sha256};
use std::{future::Future, pin::Pin, time::Duration};

/// Canonical fixed-corpus correctness source.
pub const HTTP_CORPUS_SOURCE: &str = "eggbench-http-corpus";
/// Semantic version of the Eggbench corpus-to-HTTP mapping.
pub const HTTP_CORPUS_SEMANTIC_VERSION: &str = "eggbench-http-corpus.v1";
const MAX_RESPONSE_BODY_BYTES: usize = 8 * 1024 * 1024;

/// Eggfetch backed, serial HTTP corpus executor.
#[derive(Debug, Default)]
pub struct HttpCorpusExecutor;

impl CorrectnessExecutor for HttpCorpusExecutor {
    fn source(&self) -> &str {
        HTTP_CORPUS_SOURCE
    }

    fn execute<'a>(
        &'a mut self,
        context: CorrectnessContext,
    ) -> Pin<Box<dyn Future<Output = Result<CorrectnessOutput, FailureCategory>> + Send + 'a>> {
        Box::pin(async move { execute_corpus(context).await })
    }
}

#[allow(clippy::too_many_lines)]
async fn execute_corpus(context: CorrectnessContext) -> Result<CorrectnessOutput, FailureCategory> {
    let request = context
        .http_corpus_request
        .as_ref()
        .ok_or(FailureCategory::CorrectnessFailed)?;
    let binding = context
        .bindings
        .get(request.target.as_str(), "http_url")
        .ok_or(FailureCategory::CorrectnessFailed)?;
    let _confined = crate::external::confine_target_url(binding)
        .map_err(|_| FailureCategory::CorrectnessFailed)?;
    let scheme = binding.split_once("://").map_or("", |(scheme, _)| scheme);
    let authority_start = binding
        .find("://")
        .ok_or(FailureCategory::CorrectnessFailed)?
        + 3;
    let authority_end = binding[authority_start..]
        .find(['/', '?', '#'])
        .map_or(binding.len(), |offset| authority_start + offset);
    let origin = format!("{scheme}://{}", &binding[authority_start..authority_end]);
    if origin.contains('@') {
        return Err(FailureCategory::CorrectnessFailed);
    }
    let workspace = std::env::current_dir().map_err(|_| FailureCategory::CorrectnessFailed)?;
    let (corpus, body_root) =
        load_http_security_corpus(&workspace, &request.corpus_ref, &request.corpus_sha256)
            .map_err(|_| FailureCategory::CorrectnessFailed)?;
    let client = eggfetch_core::Client::new();
    let deadline = std::time::Instant::now()
        .checked_add(context.timeout)
        .unwrap_or_else(std::time::Instant::now);
    let mut cases = Vec::with_capacity(corpus.cases.len());
    for case in &corpus.cases {
        if context.cancellation.is_cancelled() {
            return Err(FailureCategory::Cancelled);
        }
        let case_bytes =
            serde_json::to_vec(case).map_err(|_| FailureCategory::CorrectnessFailed)?;
        let case_sha256 = format!("{:x}", Sha256::digest(case_bytes));
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            cases.push(HttpCorpusCaseResultV1 {
                id: case.id.clone(),
                case_sha256,
                expectation: case.expectation.clone(),
                observed_status: None,
                disposition: HttpCorpusCaseDisposition::Invalid,
                reason: Some("check_timeout".to_owned()),
            });
            continue;
        }
        let case_timeout = Duration::from_millis(request.case_timeout_ms.get()).min(remaining);
        let method = eggfetch_core::Method::from_bytes(case.request.method.as_bytes())
            .map_err(|_| FailureCategory::CorrectnessFailed)?;
        let url = format!("{origin}{}", case.request.path_and_query);
        let mut builder = client
            .request(method, &url)
            .map_err(|_| FailureCategory::CorrectnessFailed)?
            .timeout(eggfetch_core::Timeout {
                pool: Some(case_timeout),
                connect: Some(case_timeout),
                write: Some(case_timeout),
                read: Some(case_timeout),
                total: Some(case_timeout),
            })
            .max_decoded_body_size(MAX_RESPONSE_BODY_BYTES);
        for (name, value) in &case.request.headers {
            builder = builder.header(name, value);
        }
        let body = match &case.request.body {
            HttpCaseBodyV1::None => None,
            HttpCaseBodyV1::InlineUtf8(value) => Some(value.as_bytes().to_vec()),
            HttpCaseBodyV1::File(path) => {
                let identity = eggbench_core::content_tree_identity(&body_root, path)
                    .map_err(|_| FailureCategory::CorrectnessFailed)?;
                let file = body_root.join(path);
                let data = std::fs::read(file).map_err(|_| FailureCategory::CorrectnessFailed)?;
                if identity.file_count != 1
                    || identity.total_bytes != data.len() as u64
                    || identity.files[0].sha256 != format!("{:x}", Sha256::digest(&data))
                {
                    return Err(FailureCategory::CorrectnessFailed);
                }
                Some(data)
            }
        };
        if let Some(body) = body {
            builder = builder.bytes(body);
        }
        let response = tokio::select! {
            () = context.cancellation.cancelled() => return Err(FailureCategory::Cancelled),
            response = builder.send_detailed() => response,
        };
        let Ok(mut response) = response else {
            cases.push(HttpCorpusCaseResultV1 {
                id: case.id.clone(),
                case_sha256,
                expectation: case.expectation.clone(),
                observed_status: None,
                disposition: HttpCorpusCaseDisposition::Invalid,
                reason: Some(if std::time::Instant::now() >= deadline {
                    "check_timeout".to_owned()
                } else {
                    "transport_failure".to_owned()
                }),
            });
            continue;
        };
        let status = response.status().as_u16();
        tokio::select! {
            () = context.cancellation.cancelled() => return Err(FailureCategory::Cancelled),
            _body = response.bytes() => {}
        }
        let disposition = if case.expectation.matches(status) {
            HttpCorpusCaseDisposition::Pass
        } else {
            HttpCorpusCaseDisposition::Fail
        };
        cases.push(HttpCorpusCaseResultV1 {
            id: case.id.clone(),
            case_sha256,
            expectation: case.expectation.clone(),
            observed_status: Some(status),
            disposition,
            reason: None,
        });
    }
    let result = HttpCorpusCheckResultV1 {
        schema_version: 1,
        id: request.id.as_str().to_owned(),
        source: request.source.as_str().to_owned(),
        family: "http_observable".to_owned(),
        target: request.target.as_str().to_owned(),
        corpus_id: corpus.corpus_id,
        corpus_sha256: request.corpus_sha256.to_ascii_lowercase(),
        adapter_semantic_version: HTTP_CORPUS_SEMANTIC_VERSION.to_owned(),
        eggfetch_version: super::EGGFETCH_CORE_VERSION.to_owned(),
        cases,
    };
    result
        .validate_contract()
        .map_err(|_| FailureCategory::CorrectnessFailed)?;
    let (evaluated, _passed, failed, _invalid) = result.counts();
    let producer = format!(
        "{}:{}",
        HTTP_CORPUS_SEMANTIC_VERSION,
        super::EGGFETCH_CORE_VERSION
    );
    let producer_sha256 = format!("{:x}", Sha256::digest(producer.as_bytes()));
    Ok(CorrectnessOutput {
        disposition: if failed == 0 {
            CorrectnessDisposition::Pass
        } else {
            CorrectnessDisposition::Fail
        },
        sanitized_result: serde_json::to_vec(&result)
            .map_err(|_| FailureCategory::CorrectnessFailed)?,
        producer: "eggfetch".to_owned(),
        producer_version: producer,
        executable_sha256: producer_sha256,
        scope_sha256: request.corpus_sha256.to_ascii_lowercase(),
        evaluated_cases: evaluated,
        successful_bypasses: failed,
    })
}
