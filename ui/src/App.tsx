import { ThemePicker } from "suyo-ui";
import { Link, useRoute, type Route } from "./router";
import { TodayPage } from "./pages/Today";
import { DashboardPage } from "./pages/DashboardPage";
import { ActivitiesPage } from "./pages/Activities";
import { ActivityPage } from "./pages/Activity";
import { ModelPage } from "./pages/Model";
import { DataPage } from "./pages/Data";
import { WeekPage } from "./pages/Week";

const NAV: { label: string; to: string; page: Route["page"] }[] = [
  { label: "Today", to: "/", page: "today" },
  { label: "Week", to: "/week", page: "week" },
  { label: "Trends", to: "/trends", page: "trends" },
  { label: "Activities", to: "/activities", page: "activities" },
  { label: "Model", to: "/model", page: "model" },
  { label: "Data", to: "/data", page: "data" },
];

function Header() {
  const { route } = useRoute();
  const active = route.page === "activity" ? "activities" : route.page;
  return (
    <header className="topbar">
      <Link to="/" className="brand">
        <img src="/favicon.svg" alt="" width={20} height={20} />
        eenkah
      </Link>
      <nav className="nav" aria-label="Views">
        {NAV.map((n) => (
          <Link key={n.to} to={n.to} className={"nav-link" + (active === n.page ? " on" : "")}>
            {n.label}
          </Link>
        ))}
      </nav>
      <div className="topbar-right">
        <a className="nav-link" href="/suyo/" title="suyo composer: build any chart from any series">
          Compose
        </a>
        <ThemePicker />
      </div>
    </header>
  );
}

function Page({ route }: { route: Route }) {
  switch (route.page) {
    case "today":
      return <TodayPage />;
    case "week":
      return <WeekPage />;
    case "trends":
      return <DashboardPage name="trends" />;
    case "activities":
      return <ActivitiesPage />;
    case "activity":
      return <ActivityPage id={route.id} />;
    case "model":
      return <ModelPage />;
    case "data":
      return <DataPage />;
    case "missing":
      return (
        <main className="page">
          <h1>Not found</h1>
          <p>
            Nothing at <code>{route.path}</code>. <Link to="/">Back to Today</Link>
          </p>
        </main>
      );
  }
}

export function App() {
  const { route } = useRoute();
  return (
    <>
      <Header />
      <Page route={route} />
      <footer className="footer">eenkah runs on this computer only. Not medical advice. Not affiliated with Garmin.</footer>
    </>
  );
}
