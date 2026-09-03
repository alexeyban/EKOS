import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { api, apiPost, apiPut } from "../api/client";
import type { ConfigOut, PreviewScan, ValidateResult, WriteResult } from "../api/types";

export function Config() {
  const { id = "" } = useParams();
  const qc = useQueryClient();

  const current = useQuery({
    queryKey: ["config", id],
    queryFn: () => api<ConfigOut>(`/workspaces/${id}/config`),
    enabled: id !== "",
  });

  const [text, setText] = useState("");
  const [validated, setValidated] = useState<ValidateResult | null>(null);
  const [dirtySinceValidate, setDirty] = useState(true);

  useEffect(() => {
    if (current.data) setText(current.data.raw);
  }, [current.data]);

  const validate = useMutation({
    mutationFn: () => apiPost<ValidateResult>(`/workspaces/${id}/config/validate`, { raw: text }),
    onSuccess: (r) => {
      setValidated(r);
      setDirty(false);
    },
  });

  const scan = useMutation({
    mutationFn: () => apiPost<PreviewScan>(`/workspaces/${id}/config/preview-scan`, {}),
  });

  const save = useMutation({
    mutationFn: () => apiPut<WriteResult>(`/workspaces/${id}/config`, { raw: text }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["config", id] });
      qc.invalidateQueries({ queryKey: ["ws", id] });
    },
  });

  const canSave = validated?.ok === true && !dirtySinceValidate && !save.isPending;

  return (
    <>
      <p>
        <Link to={`/w/${id}`} className="linkish">
          ← dashboard
        </Link>
      </p>

      {current.isError && <p className="err">{String(current.error)}</p>}

      <section className="card">
        <strong>ekos.toml</strong>
        <textarea
          className="toml"
          spellCheck={false}
          value={text}
          onChange={(e) => {
            setText(e.target.value);
            setDirty(true);
          }}
        />
        <div className="btnrow">
          <button onClick={() => validate.mutate()} disabled={validate.isPending}>
            Validate
          </button>
          <button onClick={() => scan.mutate()} disabled={scan.isPending}>
            Preview scan
          </button>
          <button className="save" onClick={() => save.mutate()} disabled={!canSave}>
            Save
          </button>
          {!canSave && validated?.ok && dirtySinceValidate && (
            <span className="muted">re-validate before saving</span>
          )}
        </div>

        {validate.isError && <p className="err">{String(validate.error)}</p>}
        {validated && (
          <div className="findings">
            {validated.ok && validated.warnings.length === 0 && (
              <p className="ok-line">valid ✓</p>
            )}
            {validated.errors.map((f, i) => (
              <p key={`e${i}`} className="err">
                {f.code}: {f.detail}
              </p>
            ))}
            {validated.warnings.map((f, i) => (
              <p key={`w${i}`} className="warn-line">
                {f.code}: {f.detail}
              </p>
            ))}
          </div>
        )}

        {save.isError && <p className="err">{String(save.error)}</p>}
        {save.data?.append_only_warning && (
          <div className="banner">{save.data.append_only_warning}</div>
        )}
        {save.data && !save.data.append_only_warning && save.isSuccess && (
          <p className="ok-line">saved · ekos.toml.bak written</p>
        )}
      </section>

      {scan.data && (
        <section className="card">
          <strong>Preview scan</strong>
          <p className="muted">
            {scan.data.total_files.toLocaleString()} files ·{" "}
            {(scan.data.total_bytes / 1e6).toFixed(1)} MB · {scan.data.elapsed_ms} ms
            {scan.data.truncated && " · truncated"}
          </p>
          <ul className="scan">
            {scan.data.by_extension.slice(0, 10).map((e) => (
              <li key={e.ext}>
                <code>{e.ext || "(none)"}</code> — {e.files}
              </li>
            ))}
          </ul>
          {scan.data.ignored_dir_hits
            .filter((h) => h.dirs_skipped === 0)
            .map((h) => (
              <p key={h.pattern} className="warn-line">
                ignore-pattern <code>{h.pattern}</code> matched no directories
              </p>
            ))}
        </section>
      )}

      {current.data && (
        <section className="card">
          <strong>[observe] (read-only)</strong>
          <p className="muted">paths</p>
          <div className="chips">
            {current.data.observe.paths.length === 0 ? (
              <span className="muted">— (whole workspace)</span>
            ) : (
              current.data.observe.paths.map((p) => (
                <span key={p} className="chip">
                  {p}
                </span>
              ))
            )}
          </div>
          <p className="muted">ignore-patterns</p>
          <div className="chips">
            {current.data.observe.ignore_patterns.map((p) => (
              <span key={p} className="chip">
                {p}
              </span>
            ))}
          </div>
        </section>
      )}
    </>
  );
}
