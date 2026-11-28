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
        NotSupported,
        EkosError,
        "This access mode cannot serve the call."
    );

    #[pymodule_export]
    use super::_native::{EkosError as EkosErrorT, LedgerError as LedgerErrorT};
    #[pymodule_export]
    use super::_native::{NotSupported as NotSupportedT, QueryError as QueryErrorT};

    fn raise(e: EngineError) -> PyErr {
        match e {
            EngineError::Ledger(m) => LedgerError::new_err(m),
            EngineError::NotSupported(m) => NotSupported::new_err(m),
            EngineError::Query(m) => QueryError::new_err(m),
        }
    }

    fn text(value: Result<serde_json::Value, EngineError>) -> PyResult<String> {
        match value {
            Ok(v) => Ok(v.to_string()),
            Err(e) => Err(raise(e)),
        }
    }

    /// An open, read-only ledger. One call at a time; the interpreter lock is released meanwhile.
    #[pyclass(frozen)]
    struct Ledger(Engine);

    #[pymethods]
    impl Ledger {
        fn status(&self, py: Python<'_>) -> PyResult<String> {
            py.detach(|| text(self.0.status()))
        }

        fn search(
            &self,
            py: Python<'_>,
            query: &str,
            mode: &str,
            limit: usize,
        ) -> PyResult<String> {
            py.detach(|| text(self.0.search(query, mode, limit)))
        }

        fn refresh(&self, py: Python<'_>) -> PyResult<bool> {
            py.detach(|| self.0.refresh()).map_err(raise)
        }

        fn get(&self, py: Python<'_>, object_id: &str) -> PyResult<String> {
            py.detach(|| text(self.0.get(object_id)))
        }
    }

    /// Open the ledger at `path` read-only.
    #[pyfunction]
    fn open(py: Python<'_>, path: PathBuf) -> PyResult<Ledger> {
        py.detach(|| Engine::open(&path)).map(Ledger).map_err(raise)
    }

    /// The engine version this wheel embeds.
    #[pyfunction]
    fn engine_version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }
}
