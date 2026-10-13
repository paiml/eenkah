import { api } from "../api";
import { fmt, useLoad } from "../hooks";
import { DashboardPage } from "./DashboardPage";

/** This week's training time by session type (model 6), above the week dashboard. */
function TimeByType() {
  const [week] = useLoad(api.week);
  if (week.state !== "ok") return null;
  const rows = week.data.minutes_by_type;
  const total = rows.reduce((a, r) => a + r.minutes, 0);
  if (!total) return null;
  return (
    <section className="card panel">
      <div className="panel-title">What kind of training was this week?</div>
      <table className="data-table compact">
        <tbody>
          {rows.map((r) => (
            <tr key={r.label}>
              <td>{r.label}</td>
              <td className="bar-cell">
                <span className="hbar learned" style={{ width: `${(r.minutes / total) * 100}%` }} />
              </td>
              <td className="num">{fmt(r.minutes, 0, "min")}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <p className="small muted">
        The last 7 days, including today. Session types are learned from your heart-rate shapes; correct any session on its page.
      </p>
    </section>
  );
}

export function WeekPage() {
  return <DashboardPage name="week" intro={<TimeByType />} />;
}
