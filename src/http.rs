use crate::error::{Error, Result};
use std::io::Read;
use std::time::Duration;

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(30))
        .timeout_read(Duration::from_secs(120))
        .timeout_write(Duration::from_secs(120))
        .redirects(5)
        .user_agent(concat!("pkg-merge/", env!("CARGO_PKG_VERSION")))
        .build()
}

pub fn get_bytes(url: &str, max_bytes: u64, error_code: i32) -> Result<Vec<u8>> {
    let response = agent()
        .get(url)
        .call()
        .map_err(|error| map_error(error, error_code, url))?;
    let mut reader = response.into_reader().take(max_bytes + 1);
    let mut data = Vec::new();
    reader
        .read_to_end(&mut data)
        .map_err(|e| Error::new(error_code, format!("error leyendo {url}: {e}")))?;
    if data.len() as u64 > max_bytes {
        return Err(Error::new(
            error_code,
            format!("la respuesta de {url} supera el límite de {max_bytes} bytes"),
        ));
    }
    Ok(data)
}

pub struct Download {
    pub len: Option<u64>,
    pub reader: Box<dyn Read + Send + Sync + 'static>,
}

pub fn get(url: &str, error_code: i32) -> Result<Download> {
    let response = agent()
        .get(url)
        .call()
        .map_err(|error| map_error(error, error_code, url))?;
    let len = response
        .header("Content-Length")
        .and_then(|value| value.parse::<u64>().ok());
    Ok(Download {
        len,
        reader: response.into_reader(),
    })
}

fn map_error(error: ureq::Error, code: i32, url: &str) -> Error {
    match error {
        ureq::Error::Status(status, _) => {
            Error::new(code, format!("HTTP {status} al descargar {url}"))
        }
        ureq::Error::Transport(transport) => Error::new(
            code,
            format!("error de red al descargar {url}: {transport}"),
        ),
    }
}
