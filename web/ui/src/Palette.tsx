// RFC 0261 §2.7 (U2): the command palette, Ctrl/⌘ K. One box that goes to a page, switches
// project, finds an object (opening the graph focused on it) and runs a command. Commands come
// from the workspace's catalogue (`GET /workspaces/{id}/cli`), so only what the caller may run is
// offered, and choosing one opens its form: nothing runs from the palette itself.

import { useQuery } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { api } from "./api/client";
import type { Workspace } from "./api/types";
import { Icon, type IconName } from "./Icon";
import { allItems, type Identity, type NavGroup } from "./nav";
import type { CommandEntry } from "./pages/Commands";

export interface PaletteRow {
  key: string;
  group: "Go to" | "Projects" | "Objects" | "Run a command";
  icon: IconName;
  label: string;
  detail: string;
  /** A short label at the right: a command's class, "open in Graph". */
  tag?: string;
  tagClass?: string;
  to: string;
}

const MAX_PER_GROUP = 6;

/** How well `text` matches `q` (lower is better), or null. Prefix beats word start beats inside. */
export function rank(text: string, q: string): number | null {
  const t = text.toLowerCase();
  const n = q.trim().toLowerCase();
  if (!n) return 3;
  if (t.startsWith(n)) return 0;
  if (t.split(/[\s._:/-]+/).some((w) => w.startsWith(n))) return 1;
  if (t.includes(n)) return 2;
  return null;
}

const CLASS_DETAIL: Record<CommandEntry["class"], string> = {
  read: "opens the form",
  write: "opens the form",
  review: "a decision recorded in your name",
  admin: "opens the form · asks you to type its name",
  external: "opens the form · reaches a live system",
};

/** The palette's rows for `q`, grouped and ranked. Pure, so it is tested without a DOM. */
export function paletteRows(input: {
  q: string;
  workspace?: string;
  pages: NavGroup[];
  projects: { id: string; name: string }[];
  commands: CommandEntry[];
  objects: { id: string; name: string; kind?: string }[];
}): PaletteRow[] {
  const { q, workspace } = input;
  const best = <T,>(items: T[], text: (t: T) => string) =>
    items
      .map((item) => ({ item, r: rank(text(item), q) }))
      .filter((x): x is { item: T; r: number } => x.r !== null)
      .sort((a, b) => a.r - b.r)
      .slice(0, MAX_PER_GROUP)
      .map((x) => x.item);

  const pages: PaletteRow[] = best(allItems(input.pages), (p) => p.label).map((p) => ({
    key: `page:${p.to}`,
    group: "Go to",
    icon: p.icon,
    label: p.label,
    detail: p.group,
    to: p.to,
  }));
  const projects: PaletteRow[] = best(
    input.projects.filter((p) => p.id !== workspace),
    (p) => p.name,
  ).map((p) => ({
    key: `project:${p.id}`,
    group: "Projects",
    icon: "projects",
    label: p.name,
    detail: "switch project",
    to: `/w/${p.id}`,
  }));
  const objects: PaletteRow[] =
    workspace === undefined
      ? []
      : input.objects.slice(0, MAX_PER_GROUP).map((o) => ({
          key: `object:${o.id}`,
          group: "Objects",
          icon: "graph",
          label: o.name,
          detail: o.kind ?? "object",
          tag: "open in Graph",
          to: `/w/${workspace}/graph?focus=${encodeURIComponent(o.id)}`,
        }));
  const commands: PaletteRow[] =
    workspace === undefined || !q.trim()
      ? []
      : best(
          input.commands.filter((c) => c.allowed),
          (c) => c.path.join(" "),
        ).map((c) => ({
          key: `command:${c.path.join(" ")}`,
          group: "Run a command",
          icon: "run",
          label: c.path.join(" "),
          detail: CLASS_DETAIL[c.class] + (c.metered ? " · may call the configured model" : ""),
          tag: c.class,
          tagClass: c.class === "read" ? "chip" : c.class === "write" ? "chip accent" : "chip warn",
          to: `/w/${workspace}/commands?cmd=${encodeURIComponent(c.path.join(" "))}`,
        }));
  return [...pages, ...projects, ...objects, ...commands];
}

/** Opens the palette on Ctrl/⌘ K anywhere in the console. */
export function usePaletteHotkey() {
  const [open, setOpen] = useState(false);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setOpen((o) => !o);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  return { open, setOpen };
}

export function CommandPalette({
  me,
  workspace,
  groups,
  onClose,
}: {
  me: Identity;
  workspace: string | undefined;
  groups: NavGroup[];
  onClose: () => void;
}) {
  const [q, setQ] = useState("");
  const [sel, setSel] = useState(0);
  const navigate = useNavigate();
  const dialog = useRef<HTMLDialogElement>(null);
  const projects = useQuery({ queryKey: ["workspaces"], queryFn: () => api<Workspace[]>("/workspaces") });
  // A restricted reader and a read-only showcase run no commands (RFC 0181, RFC 0251).
  const restricted = projects.data?.find((w) => w.id === workspace)?.restricted === true;
  const commands = useQuery({
    queryKey: ["cli", workspace],
    queryFn: () => api<{ commands: CommandEntry[] }>(`/workspaces/${workspace}/cli`),
    enabled: workspace !== undefined && !restricted && me.read_only !== true,
    staleTime: 300_000,
  });
  const term = q.trim();
  const objects = useQuery({
    queryKey: ["psearch", workspace, term],
    queryFn: () =>
      api<{ matches: { id: string; name: string; kind?: string }[] }>(
        `/workspaces/${workspace}/search?q=${encodeURIComponent(term)}&limit=6`,
      ),
    enabled: workspace !== undefined && term.length > 1,
    staleTime: 60_000,
  });

  const rows = useMemo(
    () =>
      paletteRows({
        q,
        workspace,
        pages: groups,
        projects: projects.data ?? [],
        commands: commands.data?.commands ?? [],
        objects: objects.data?.matches ?? [],
      }),
    [q, workspace, groups, projects.data, commands.data, objects.data],
  );

  useEffect(() => {
    const d = dialog.current;
    if (!d) return;
    if (!d.open) d.showModal?.();
    // A click on the backdrop lands on the dialog itself. Escape is the keyboard way out (onCancel).
    const onBackdrop = (e: MouseEvent) => {
      if (e.target === d) onClose();
    };
    d.addEventListener("click", onBackdrop);
    return () => d.removeEventListener("click", onBackdrop);
  }, [onClose]);
  useEffect(() => setSel(0), [q]);

  const go = (row: PaletteRow | undefined) => {
    if (!row) return;
    onClose();
    navigate(row.to);
  };

  let lastGroup = "";
  return (
    <dialog
      ref={dialog}
      className="palette"
      aria-label="Search or run a command"
      onCancel={(e) => {
        e.preventDefault();
        onClose();
      }}
    >
      <div className="palette-input">
        <Icon name="search" size={16} />
        <input
          autoFocus
          value={q}
          placeholder="Search pages, objects and commands"
          aria-label="Search pages, objects and commands"
          aria-controls="palette-list"
          aria-activedescendant={rows[sel] ? `pr-${sel}` : undefined}
          onChange={(e) => setQ(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown") {
              e.preventDefault();
              setSel((s) => Math.min(s + 1, rows.length - 1));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setSel((s) => Math.max(s - 1, 0));
            } else if (e.key === "Enter") {
              e.preventDefault();
              go(rows[sel]);
            }
          }}
        />
        <kbd>Esc</kbd>
      </div>
      <ul className="palette-list" id="palette-list" role="listbox">
        {rows.length === 0 && <li className="palette-empty muted">Nothing matches “{term}”.</li>}
        {rows.map((row, i) => {
          const head = row.group !== lastGroup ? row.group : null;
          lastGroup = row.group;
          return (
            <li key={row.key} role="presentation">
              {head && <div className="palette-group">{head}</div>}
              <div
                id={`pr-${i}`}
                role="option"
                aria-selected={i === sel}
                tabIndex={-1}
                className={`palette-row${i === sel ? " active" : ""}`}
                onMouseMove={() => setSel(i)}
                onClick={() => go(row)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") go(row);
                }}
              >
                <Icon name={row.icon} />
                <span className="palette-label">{row.label}</span>
                <span className="palette-detail">{row.detail}</span>
                {row.tag && <span className={row.tagClass ?? "chip"}>{row.tag}</span>}
              </div>
            </li>
          );
        })}
      </ul>
      <div className="palette-foot">
        <span>
          <kbd>↑</kbd> <kbd>↓</kbd> move <kbd>Enter</kbd> open
        </span>
        <span>Only commands you may run are listed. Nothing runs without its form.</span>
      </div>
    </dialog>
  );
}
