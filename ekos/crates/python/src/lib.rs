//! RFC 0269 — the native read engine of the `ekos` Python package.
//!
//! [`engine`] is plain Rust; the PyO3 glue is behind the `python` feature so the workspace builds
//! and tests without a Python interpreter. The glue hands results to Python as JSON text: the
//! dataclasses in `python/ekos/_types.py` are the public contract, not these Rust types.

pub mod engine;

#[cfg(feature = "python")]
#[pyo3::pymodule]
mod _native {
    use crate::engine::{Engine, EngineError};
    use pyo3::create_exception;
    use pyo3::exceptions::PyException;
    use pyo3::prelude::*;
    use std::path::PathBuf;

    create_exception!(_native, EkosError, PyException, "Base of every EKOS error.");
    create_exception!(
        _native,
        LedgerError,
        EkosError,
        "The ledger cannot be opened or read."
    );
    create_exception!(
        _native,
        QueryError,
        EkosError,
        "The query or argument is wrong."
    );
    create_exception!(
        _native,
        NotFound,
        QueryError,
        "The id is well-formed and names nothing in the ledger."
    );
    create_exception!(
        _native,
        NotSupported,
        EkosError,
        "This access mode cannot serve the call."
    );

    #[pymodule_export]
    use super::_native::QueryError as QueryErrorT;
    #[pymodule_export]
    use super::_native::{EkosError as EkosErrorT, LedgerError as LedgerErrorT};
    #[pymodule_export]
    use super::_native::{NotFound as NotFoundT, NotSupported as NotSupportedT};

    fn raise(e: EngineError) -> PyErr {
        match e {
            EngineError::Ledger(m) => LedgerError::new_err(m),
            EngineError::NotSupported(m) => NotSupported::new_err(m),
            EngineError::Query(m) => QueryError::new_err(m),
            EngineError::NotFound(m) => NotFound::new_err(m),
        }
    }

    /// An open, read-only ledger. One call at a time; the interpreter lock is released meanwhile.
    #[pyclass(frozen)]
    struct Ledger(Engine);

    #[pymethods]
    impl Ledger {
        /// Run the read `name` with JSON `arguments`; the answer is JSON text.
        #[pyo3(signature = (name, arguments, embedding=None))]
        fn call(
            &self,
            py: Python<'_>,
            name: &str,
            arguments: &str,
            embedding: Option<Vec<f32>>,
        ) -> PyResult<String> {
            py.detach(|| {
                let args: serde_json::Value = serde_json::from_str(arguments)
                    .map_err(|e| EngineError::Query(format!("arguments are not JSON: {e}")))?;
                Ok(self.0.call(name, &args, embedding)?.to_string())
            })
            .map_err(raise)
        }

        /// What the caller should know about this store, in words.
        fn warnings(&self) -> Vec<String> {
            self.0.warnings().to_vec()
        }

        fn refresh(&self, py: Python<'_>) -> PyResult<bool> {
            py.detach(|| self.0.refresh()).map_err(raise)
        }
    }

    /// Open the ledger at `path` read-only. `read_scope` is an RFC 0181 scope as JSON.
    #[pyfunction]
    #[pyo3(signature = (path, read_scope=None))]
    fn open(py: Python<'_>, path: PathBuf, read_scope: Option<String>) -> PyResult<Ledger> {
        py.detach(|| Engine::open(&path, read_scope.as_deref()))
            .map(Ledger)
            .map_err(raise)
    }

    /// The reads this engine serves, by name.
    #[pyfunction]
    fn reads() -> Vec<&'static str> {
        crate::engine::READS.iter().map(|r| r.name).collect()
    }

    /// The engine version this wheel embeds.
    #[pyfunction]
    fn engine_version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }
}
