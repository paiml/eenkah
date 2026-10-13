// Top bar. When suyo is embedded in a host server, window.SUYO_HOST supplies the brand and
// the host's own nav links (absolute paths on the host); suyo's links follow them.

import type { ReactNode } from "react";
import { url } from "../lib";

export function Header({ active, children }: { active: string; children?: ReactNode }) {
  const host = window.SUYO_HOST ?? {};
  const own: [string, string][] = [["/", "Compose"], ["/d", "Dashboards"], ["/docs", "Docs"]];
  const ownLinks = own.map(([href, label]) => (
    <a key={href} href={url(href)} className={"nav-link" + (active === href ? " on" : "")}>{label}</a>
  ));
  const here = window.location.pathname;
  const hereQ = here + window.location.search;
  const hostNav = (host.nav ?? []).map(([label, href]) => (
    <a key={href + label} href={href} className={"nav-link" + (hereQ === href || here === href ? " on" : "")}>{label}</a>
  ));
  const [brandName, brandHref] = host.brand ?? ["suyo", url("/")];
  return (
    <header className="topbar">
      <a href={brandHref} className="brand" aria-label={`${brandName} home`}>
        <span className="brand-mark" aria-hidden="true" />
        {brandName}
      </a>
      <nav className="nav">
        {hostNav.length ? (
          <>
            {hostNav}
            <span className="nav-sep" aria-hidden="true" />
            {ownLinks.slice(0, 2)}
          </>
        ) : (
          ownLinks
        )}
      </nav>
      <div className="topbar-right">{children}</div>
    </header>
  );
}
