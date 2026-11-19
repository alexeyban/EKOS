import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { useParams } from "react-router-dom";
import { api, apiPost, apiPut } from "../api/client";
import type { ConfigOut, PreviewScan, ValidateResult, WriteResult } from "../api/types";
import { ErrorNote, PageHead, Skeleton } from "../ui";
import { ConfigForm } from "./ConfigForm";

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
      void qc.invalidateQueries({ queryKey: ["config", id] });
      void qc.invalidateQueries({ queryKey: ["ws", id] });
    },
  });

  const canSave = validated?.ok === true && !dirtySinceValidate && !save.isPending;

  return (
    <div className="fm-page">
      <PageHead
        title="ekos.toml"
        sub="This project's configuration: change a setting in the form, or edit the file itself, then validate and save."
      />
      {current.isError && <ErrorNote error={current.error} what="Reading ekos.toml" />}
      {current.isPending && <Skeleton rows={6} label="loading ekos.toml" />}

      {current.data && (
        <ConfigForm
          id={id}
          raw={text}
          values={current.data.values ?? {}}
          onApplied={(raw) => {
            setText(raw);
            setDirty(true);
            setValidated(null);
          }}
        />
      )}

      <section className="card" aria-labelledby="cfg-file">
        <h2 className="card-label" id="cfg-file">
          ekos.toml
        </h2>
        <textarea
          className="fm-editor"
          aria-label="ekos.toml text"
          spellCheck={false}
          value={text}
          onChange={(e) => {
            setText(e.target.value);
            setDirty(true);
          }}
        />
        {validated && <Findings validated={validated} />}
        <div className="fm-actions">
          <button className="primary" onClick={() => save.mutate()} disabled={!canSave}>
            Save
          </button>
          <button onClick={() => validate.mutate()} disabled={validate.isPending}>
            Validate
          </button>
          <button onClick={() => scan.mutate()} disabled={scan.isPending}>
            Preview scan
          </button>
          {!canSave && validated?.ok && dirtySinceValidate && (
            <span className="fm-status">re-validate before saving</span>
          )}
          {!validated && <span className="fm-status">Validate, then save.</span>}
        </div>

        {validate.isError && <ErrorNote error={validate.error} what="Validating" />}
        {save.isError && <ErrorNote error={save.error} what="Saving" />}
        {save.data?.append_only_warning && (
          <div className="fm-banner">{save.data.append_only_warning}</div>
        )}
        {save.data && !save.data.append_only_warning && save.isSuccess && (
          <p className="fm-ok">saved · ekos.toml.bak written</p>
        )}
      </section>

      {scan.data && <ScanResult scan={scan.data} />}

      {current.data && <Observe observe={current.data.observe} />}
    </div>
  );
}

/** What validation found, right under the text it is about. */
function Findings({ validated }: { validated: ValidateResult }) {
  return (
    <div className="fm-findings" role="status">
      {validated.ok && validated.warnings.length === 0 && <p className="fm-ok">valid ✓</p>}
      {validated.errors.map((f, i) => (
        <p key={`e${i}`} className="fm-error">
          {f.code}: {f.detail}
        </p>
      ))}
      {validated.warnings.map((f, i) => (
        <p key={`w${i}`} className="fm-warn">
          {f.code}: {f.detail}
        </p>
      ))}
    </div>
  );
}

function ScanResult({ scan }: { scan: PreviewScan }) {
  return (
    <section className="card" aria-labelledby="cfg-scan">
      <h2 className="card-label" id="cfg-scan">
        Preview scan
      </h2>
      <p className="fm-note">
        {scan.total_files.toLocaleString()} files · {(scan.total_bytes / 1e6).toFixed(1)} MB ·{" "}
        {scan.elapsed_ms} ms
        {scan.truncated && " · truncated"}
      </p>
      <ul className="fm-scan">
        {scan.by_extension.slice(0, 10).map((e) => (
          <li key={e.ext}>
            <code>{e.ext || "(none)"}</code> — {e.files}
          </li>
        ))}
      </ul>
      {scan.ignored_dir_hits
        .filter((h) => h.dirs_skipped === 0)
        .map((h) => (
          <p key={h.pattern} className="fm-warn">
            ignore-pattern <code>{h.pattern}</code> matched no directories
          </p>
        ))}
    </section>
  );
}

function Observe({ observe }: { observe: ConfigOut["observe"] }) {
  return (
    <section className="card" aria-labelledby="cfg-observe">
      <h2 className="card-label" id="cfg-observe">
        [observe] (read-only)
      </h2>
      <h3 className="fm-sub-title">paths</h3>
      <div className="fm-chips">
        {observe.paths.length === 0 ? (
          <span className="muted">— (whole workspace)</span>
        ) : (
          observe.paths.map((p) => (
            <span key={p} className="chip">
              {p}
            </span>
          ))
        )}
      </div>
      <h3 className="fm-sub-title">ignore-patterns</h3>
      <div className="fm-chips">
        {observe.ignore_patterns.map((p) => (
          <span key={p} className="chip">
            {p}
          </span>
        ))}
      </div>
    </section>
  );
}
