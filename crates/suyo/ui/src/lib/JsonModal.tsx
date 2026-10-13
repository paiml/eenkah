// Modal JSON editor for dashboard specs.

import { useEffect, useRef, useState } from "react";
import { errorMessage } from "./store";

export interface JsonModalProps {
  text: string;
  title?: string;
  /** Resolve false to keep the modal open. Throwing shows the error. */
  onSave: (text: string) => Promise<boolean | void> | boolean | void;
  onClose: () => void;
}

export function JsonModal({ text, title = "Dashboard JSON", onSave, onClose }: JsonModalProps) {
  const [value, setValue] = useState(text);
  const [err, setErr] = useState("");
  const ta = useRef<HTMLTextAreaElement>(null);
  useEffect(() => ta.current?.focus(), []);
  const save = async () => {
    try {
      JSON.parse(value);
    } catch (e) {
      setErr("Invalid JSON: " + errorMessage(e));
      return;
    }
    try {
      const r = await onSave(value);
      if (r !== false) onClose();
    } catch (e) {
      setErr(errorMessage(e));
    }
  };
  return (
    <div className="modal-back" onClick={(e) => e.target === e.currentTarget && onClose()}>
      <div className="card modal">
        <div className="panel-title">{title}</div>
        <div className="small muted">Panels: {"{ title, subtitle, chart, targets[], from, until, step, w (1–12), h (rows), options{} }"}</div>
        <textarea ref={ta} className="textarea" spellCheck={false} value={value} onChange={(e) => setValue(e.target.value)} />
        <div className="errors">{err}</div>
        <div className="row gap" style={{ justifyContent: "flex-end" }}>
          <button className="btn" onClick={onClose}>Cancel</button>
          <button className="btn primary" onClick={save}>Save</button>
        </div>
      </div>
    </div>
  );
}
