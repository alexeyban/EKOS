// RFC 0261 §2.7, the page pass for the Decide pages (Review, Semantics, Conflicts, Questions,
// Inbox, Notes): one list-and-detail pattern. A list card whose rows carry status chips, the
// selected row marked, and a detail panel on the right that stays in view and scrolls on its own
// (it stacks under the list on a phone). The styles are in `styles/decide.css`.

import type { ReactNode } from "react";
import { Icon } from "../Icon";

/** The list and, when something is selected, its detail panel beside it. */
export function SplitView({ panel, children }: { panel?: ReactNode; children: ReactNode }) {
  const open = panel != null && panel !== false;
  return (
    <div className={open ? "dk-split has-panel" : "dk-split"}>
      {children}
      {open && panel}
    </div>
  );
}

/** One row of a list: chips before the name, the name as the button that selects the row (its
 * hit area covers the whole row), chips after it, and a muted line under it. */
export function PickRow({
  selected,
  onPick,
  title,
  lead,
  before,
  after,
  sub,
}: {
  selected: boolean;
  onPick: () => void;
  title: ReactNode;
  /** A control that must stay clickable on its own (a selection checkbox). */
  lead?: ReactNode;
  before?: ReactNode;
  after?: ReactNode;
  sub?: ReactNode;
}) {
  return (
    <li className={selected ? "dk-row is-selected" : "dk-row"}>
      <div className="dk-row-head">
        {lead}
        {before}
        <button type="button" className="dk-row-pick" aria-pressed={selected} onClick={onPick}>
          {title}
        </button>
        {after}
      </div>
      {sub != null && sub !== "" && <div className="dk-row-sum">{sub}</div>}
    </li>
  );
}

/** The detail panel: a title row with a close button, then the content. */
export function DetailPanel({
  label,
  title,
  onClose,
  children,
}: {
  label?: string;
  title?: ReactNode;
  onClose?: () => void;
  children: ReactNode;
}) {
  return (
    <aside className="card dk-panel" aria-label={label}>
      {(title != null || onClose != null) && (
        <div className="dk-panel-head">
          <div className="dk-panel-title">{title}</div>
          {onClose && (
            <button type="button" className="icon-btn dk-close" onClick={onClose} aria-label="close">
              <Icon name="close" />
            </button>
          )}
        </div>
      )}
      {children}
    </aside>
  );
}

/** The name a decision is recorded under where nobody is signed in (token or local mode): the API
 * records it as `token:<name>` or `local:<name>`, never as an authenticated person. */
export function ReviewerField({
  value,
  onChange,
  ariaLabel,
  who = "Reviewer",
}: {
  value: string;
  onChange: (v: string) => void;
  ariaLabel?: string;
  who?: "Reviewer" | "Your name";
}) {
  return (
    <label className="dk-field">
      <span>
        {who} (no signed-in identity here; recorded as token:&lt;name&gt; or local:&lt;name&gt;)
      </span>
      <input aria-label={ariaLabel} value={value} onChange={(e) => onChange(e.target.value)} />
    </label>
  );
}

/** `needs_review` → `needs review`. */
export function words(s: string): string {
  return s.replaceAll("_", " ");
}
