//! Classifies a call site's owner type as an external I/O boundary (RFC 0148).
//!
//! This is the one place in stage 1 that encodes domain knowledge rather than format mechanics,
//! and it is deliberately a **fixed, ordered prefix table** rather than a heuristic: an
//! `ExternalIoBoundary` fact asserts that a compiled method reaches out of the process, and that
//! claim has to be reproducible and inspectable. A pattern that is not in this table produces no
//! fact at all — silence, not a guess.
//!
//! Longest-prefix-wins, so `java.io.File` (file) is not shadowed by a hypothetical shorter
//! `java.io` entry, and `javax.jms` (messaging) is not swallowed by `javax`.

use crate::ast::IoBoundary;

/// `(type-name prefix, boundary)`, checked longest-prefix-first by [`classify`].
///
/// Both ecosystems are covered by one table because the owner names never collide: a JVM owner is
/// always `java.*`/`javax.*`/`org.*`, a CLR owner always `System.*`/`Microsoft.*`.
const PATTERNS: &[(&str, IoBoundary)] = &[
    // ── JVM: database ───────────────────────────────────────────────────────
    ("java.sql.", IoBoundary::Database),
    ("javax.sql.", IoBoundary::Database),
    ("javax.persistence.", IoBoundary::Database),
    ("jakarta.persistence.", IoBoundary::Database),
    ("org.hibernate.", IoBoundary::Database),
    ("org.springframework.jdbc.", IoBoundary::Database),
    ("org.springframework.data.", IoBoundary::Database),
    ("com.mongodb.", IoBoundary::Database),
    // ── JVM: http ───────────────────────────────────────────────────────────
    ("java.net.http.", IoBoundary::Http),
    ("java.net.URL", IoBoundary::Http),
    ("java.net.HttpURLConnection", IoBoundary::Http),
    ("javax.ws.rs.", IoBoundary::Http),
    ("jakarta.ws.rs.", IoBoundary::Http),
    ("javax.servlet.", IoBoundary::Http),
    ("jakarta.servlet.", IoBoundary::Http),
    ("org.apache.http.", IoBoundary::Http),
    ("okhttp3.", IoBoundary::Http),
    ("org.springframework.web.client.", IoBoundary::Http),
    // ── JVM: file ───────────────────────────────────────────────────────────
    ("java.io.File", IoBoundary::File),
    ("java.io.FileInputStream", IoBoundary::File),
    ("java.io.FileOutputStream", IoBoundary::File),
    ("java.io.FileReader", IoBoundary::File),
    ("java.io.FileWriter", IoBoundary::File),
    ("java.io.RandomAccessFile", IoBoundary::File),
    ("java.nio.file.", IoBoundary::File),
    // ── JVM: messaging ──────────────────────────────────────────────────────
    ("javax.jms.", IoBoundary::Messaging),
    ("jakarta.jms.", IoBoundary::Messaging),
    ("javax.naming.", IoBoundary::Messaging),
    ("org.apache.kafka.", IoBoundary::Messaging),
    ("com.rabbitmq.", IoBoundary::Messaging),
    // ── JVM: process ────────────────────────────────────────────────────────
    ("java.lang.Runtime", IoBoundary::Process),
    ("java.lang.ProcessBuilder", IoBoundary::Process),
    // ── CLR: database ───────────────────────────────────────────────────────
    // Providers and the connection/command abstractions only. Most of `System.Data` is the
    // in-memory `DataSet`/`DataTable`/`DataRow` model, which never leaves the process: a bare
    // `System.Data.` prefix claimed ~2,000 `DataTable.get_Columns`-style calls in one real app as
    // database boundaries and buried its actual ones.
    ("System.Data.SqlClient.", IoBoundary::Database),
    ("System.Data.SqlServerCe.", IoBoundary::Database),
    ("System.Data.OleDb.", IoBoundary::Database),
    ("System.Data.Odbc.", IoBoundary::Database),
    ("System.Data.OracleClient.", IoBoundary::Database),
    ("System.Data.SQLite.", IoBoundary::Database),
    ("System.Data.EntityClient.", IoBoundary::Database),
    ("System.Data.Entity.", IoBoundary::Database),
    ("System.Data.Linq.", IoBoundary::Database),
    ("System.Data.Common.DbConnection", IoBoundary::Database),
    ("System.Data.Common.DbCommand", IoBoundary::Database),
    ("System.Data.Common.DbDataReader", IoBoundary::Database),
    ("System.Data.Common.DbDataAdapter", IoBoundary::Database),
    ("System.Data.Common.DbTransaction", IoBoundary::Database),
    ("System.Data.IDbConnection", IoBoundary::Database),
    ("System.Data.IDbCommand", IoBoundary::Database),
    ("System.Data.IDataReader", IoBoundary::Database),
    ("System.Data.IDbDataAdapter", IoBoundary::Database),
    ("System.Data.IDbTransaction", IoBoundary::Database),
    ("Microsoft.Data.", IoBoundary::Database),
    ("Microsoft.EntityFrameworkCore.", IoBoundary::Database),
    ("Npgsql.", IoBoundary::Database),
    ("Oracle.ManagedDataAccess.", IoBoundary::Database),
    ("MySql.Data.", IoBoundary::Database),
    ("Dapper.", IoBoundary::Database),
    // ── CLR: http ───────────────────────────────────────────────────────────
    ("System.Net.Http.", IoBoundary::Http),
    ("System.Net.WebClient", IoBoundary::Http),
    ("System.Net.HttpWebRequest", IoBoundary::Http),
    ("System.Net.WebRequest", IoBoundary::Http),
    ("System.ServiceModel.", IoBoundary::Http),
    ("System.Web.", IoBoundary::Http),
    ("RestSharp.", IoBoundary::Http),
    // ── CLR: file ───────────────────────────────────────────────────────────
    ("System.IO.File", IoBoundary::File),
    ("System.IO.Directory", IoBoundary::File),
    ("System.IO.StreamReader", IoBoundary::File),
    ("System.IO.StreamWriter", IoBoundary::File),
    ("System.IO.FileStream", IoBoundary::File),
    // Not `System.IO.Path`: `Combine`/`GetFileName`/`GetExtension` are string manipulation and
    // touch no file.
    // ── CLR: messaging ──────────────────────────────────────────────────────
    ("System.Messaging.", IoBoundary::Messaging),
    ("Microsoft.Azure.ServiceBus.", IoBoundary::Messaging),
    ("Azure.Messaging.", IoBoundary::Messaging),
    ("RabbitMQ.Client.", IoBoundary::Messaging),
    // ── CLR: process ────────────────────────────────────────────────────────
    ("System.Diagnostics.Process", IoBoundary::Process),
];

/// The I/O boundary a call to `owner` crosses, or `None` when the table does not recognize it.
///
/// Longest match wins. The table is small and scanned linearly rather than indexed: it runs once
/// per call site, and a prefix trie here would trade real clarity for unmeasurable time.
pub fn classify(owner: &str) -> Option<IoBoundary> {
    PATTERNS
        .iter()
        .filter(|(prefix, _)| owner.starts_with(prefix))
        .max_by_key(|(prefix, _)| prefix.len())
        .map(|&(_, boundary)| boundary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jdbc_and_ado_both_classify_as_database() {
        assert_eq!(
            classify("java.sql.PreparedStatement"),
            Some(IoBoundary::Database)
        );
        assert_eq!(
            classify("System.Data.SqlClient.SqlCommand"),
            Some(IoBoundary::Database)
        );
    }

    #[test]
    fn file_http_messaging_and_process_are_distinguished() {
        assert_eq!(classify("java.io.FileOutputStream"), Some(IoBoundary::File));
        assert_eq!(classify("okhttp3.OkHttpClient"), Some(IoBoundary::Http));
        assert_eq!(
            classify("javax.jms.QueueSender"),
            Some(IoBoundary::Messaging)
        );
        assert_eq!(
            classify("java.lang.ProcessBuilder"),
            Some(IoBoundary::Process)
        );
        assert_eq!(
            classify("System.Diagnostics.Process"),
            Some(IoBoundary::Process)
        );
    }

    /// `java.io.StringWriter` is not a file, and matching a bare `java.io.` prefix would have
    /// claimed it was. The table lists concrete file types for exactly this reason.
    #[test]
    fn non_file_java_io_types_are_not_claimed_as_file_io() {
        assert_eq!(classify("java.io.StringWriter"), None);
        assert_eq!(classify("java.io.ByteArrayOutputStream"), None);
        assert_eq!(classify("java.io.PrintStream"), None);
    }

    #[test]
    fn ordinary_business_and_stdlib_types_produce_nothing() {
        assert_eq!(classify("com.acme.billing.InvoiceCalculator"), None);
        assert_eq!(classify("java.lang.String"), None);
        assert_eq!(classify("java.util.ArrayList"), None);
        assert_eq!(classify("System.Collections.Generic.List`1"), None);
        assert_eq!(classify(""), None);
    }

    /// A shorter prefix must never shadow a longer, more specific one.
    #[test]
    fn the_longest_matching_prefix_wins() {
        // `System.Web.` is http; nothing shorter may claim it first.
        assert_eq!(classify("System.Web.HttpContext"), Some(IoBoundary::Http));
        // Both `java.net.URL` and `java.net.http.` could plausibly be written as one prefix;
        // the specific entries keep each correct.
        assert_eq!(classify("java.net.URL"), Some(IoBoundary::Http));
        assert_eq!(classify("java.net.http.HttpClient"), Some(IoBoundary::Http));
    }

    /// Classification is a pure function of the owner name, so it cannot drift between runs.
    #[test]
    fn classification_is_stable_across_calls() {
        for _ in 0..3 {
            assert_eq!(
                classify("Npgsql.NpgsqlConnection"),
                Some(IoBoundary::Database)
            );
        }
    }

    /// The in-memory ADO.NET model is not I/O. `DataTable`, `DataRow` and `DataSet` live
    /// entirely in the process; only a provider or connection/command type crosses to a database.
    #[test]
    fn in_memory_ado_net_types_are_not_database_io() {
        for owner in [
            "System.Data.DataTable",
            "System.Data.DataRow",
            "System.Data.DataSet",
            "System.Data.DataColumnCollection",
            "System.Data.StrongTypingException",
        ] {
            assert_eq!(classify(owner), None, "{owner}");
        }
        for owner in [
            "System.Data.SqlServerCe.SqlCeCommand",
            "System.Data.OleDb.OleDbConnection",
            "System.Data.Common.DbDataAdapter",
            "System.Data.IDbCommand",
            "System.Data.Entity.DbContext",
        ] {
            assert_eq!(classify(owner), Some(IoBoundary::Database), "{owner}");
        }
    }

    #[test]
    fn path_manipulation_is_not_file_io() {
        assert_eq!(classify("System.IO.Path"), None);
        assert_eq!(classify("System.IO.FileStream"), Some(IoBoundary::File));
    }
}
