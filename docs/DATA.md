# Garmin files eenkah reads

Mapped from the owner's own files (Garmin Connect download layout) on 2026-10-10. The FIT contents decide what a FIT file is, so the account
export's generic file names work too [export ZIP layout unverified: no sample yet].

| File | What eenkah takes | Day it belongs to |
|---|---|---|
| `Sleep/sleep_YYYY-MM-DD.json` | `dailySleepDTO`: sleep/deep/light/REM/awake/nap seconds, `sleepScores.overall.value`, `avgSleepStress`, `averageRespirationValue`, `averageSpO2Value`, start/end GMT and Local (epoch ms). Top level: `avgOvernightHrv`, `hrvStatus` (since 2025-09), `avgSkinTempDeviationC` (since 2026-06), `restingHeartRate` | `calendarDate` = the morning the night ends on. UTC offset = Local − GMT |
| `RHR/rhr_*.json` | `allMetrics.metricsMap.WELLNESS_RESTING_HEART_RATE[].value` | `calendarDate` |
| `Weight/weight_*.json` | `dateWeightList[].weight` (grams) | `calendarDate` |
| `FitFiles/Monitoring/YYYY/daily_summary_*.json` | `bodyBatteryAtWakeTime`, `restingHeartRate`, `totalSteps`, `averageStressLevel`; UTC offset from `wellnessStart*` | `calendarDate` |
| `FitFiles/Activities/activity_<id>.json` | name, Connect type key, device training load; start GMT/Local | — |
| `*_ACTIVITY.fit` (any FIT with `session`/`record`) | session summary, per-second records (HR, speed, power, cadence, altitude, distance, temperature), `zones_target` threshold and max HR | local day of the start |
| `*_HRV_STATUS.fit` (`hrv_status_summary`) | last-night average, weekly average, balanced baseline, status: **the only HRV source before 2025-09** | local day of the summary timestamp; matches the Sleep JSON on all 359 overlapping days |
| `*_SKIN_TEMP.fit` (`skin_temp_overnight`) | `average_deviation` | date of `local_timestamp` |
| `*_SLEEP_DATA.fit` (`sleep_assessment`) | overall sleep score (fallback to the JSON) | local day of the last `sleep_level` |

Not read yet (counted in every import report):

- `WELLNESS` (per-minute monitoring).
- `METRICS` (undocumented messages; training readiness and VO2max are likely here).
- `NAP` and `SLEEP_DISRUPTIONS` (undocumented).
- Hydration (not tracked).
- Activity details JSON.

## Garmin Connect download (`eenkah import`)

Files land in `<data>/connect/inbox` using the same names as above, then the normal import
reads them.

| Data | Request | How often |
|---|---|---|
| Profile (display name) | `/userprofile-service/socialProfile` | once per data directory |
| Activities | `/activitylist-service/activities/search/activities?start&limit=100`, newest first, stops at a page with nothing new; `/download-service/files/activity/{id}` (ZIP) per new activity | per new activity |
| Wellness FIT (HRV status, sleep, skin temp, monitoring) | `/download-service/files/wellness/{day}` (ZIP) | per day not yet held |
| Sleep JSON | `/wellness-service/wellness/dailySleepData/{name}?date&nonSleepBufferMinutes=60` | per day not yet held |
| Daily summary | `/usersummary-service/usersummary/daily/{name}?calendarDate` | per day not yet held |
| Resting heart rate | `/userstats-service/wellness/daily/{name}?fromDate&untilDate&metricId=60` | one request per missing run of ≤ 90 days |
| Weight | `/weight-service/weight/dateRange?startDate&endDate` | one request per missing run of ≤ 90 days |

"Not yet held" means not already in the archive (an earlier import) and not already
downloaded. The last two days are always refreshed. Hydration is never downloaded.

Design rules for the download and import:

| Problem to avoid | eenkah |
|---|---|
| Import reads only files modified in the last 24 h | No time window: files are identified by content hash; nothing new is ever skipped |
| HRV, naps, skin temperature, Body Battery at wake discarded | All imported |
| FIT message 493 errors | Unknown messages are counted and reported, never an error |
| Fixed 1 s sleep after every request | No sleeps; back-off only on 429/5xx, honouring `Retry-After` |
| Request made before the "already downloaded" check | The archive and the download state are checked first; a final day is never requested twice |
| Weight re-downloaded from the last weigh-in | Resumes from the last day *downloaded* |
| RHR and weight one request per day | One request per missing run of up to 90 days |
| Hydration downloaded daily | Not downloaded |
