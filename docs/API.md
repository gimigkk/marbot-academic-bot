# Marbot REST API

Base URL: `https://<your-bot-host>/api/v1`. Use HTTPS outside local development.
All endpoints are read-only (`GET`; `HEAD` is also supported). This is the
**Marbot API**, not the university's API.

For a loopback-only local process, set `BIND_ADDR=127.0.0.1:3300` and
`APP_URL=http://127.0.0.1:3300`. Without `BIND_ADDR`, the existing deployment
default remains `0.0.0.0:3000`.

## API keys

Manage keys in a private WhatsApp chat with Maarbot:

| Command | Behavior |
|---|---|
| `#apikey new <name>` | Create a named key; the secret is displayed once |
| `#apikey list` | List your key names |
| `#apikey check <name>` | Show creation/last-used metadata |
| `#apikey remove <name>` | Delete your named key |
| `#apidocs` | Show the API guide in chat |

Send `Authorization: Bearer <marbot_sk_...>` on every request. Key management is
blocked in groups/channels so secrets are not sent there. Keep keys out of URLs,
client-side public code, screenshots, and shared logs.

Each key resolves to its owner. Different keys from the same owner access the
same data and share the rate-limit counter. Callers cannot supply another user,
chat, student ID, or NIM to select somebody else's attendance. Enrollment is
self-declared in WhatsApp; Marbot does not verify university-account ownership.
Existing keys can also read the owner's attendance after enrollment. Remove
keys belonging to integrations you no longer trust.

## GET /assignments

Returns the authenticated owner's active assignments, using their course settings
and completion state. Optional query filters:

| Parameter | Meaning |
|---|---|
| `timeframe` | `today`, `week`, or `all`; other values behave as `all` |
| `course` | Case-insensitive substring of course name or first alias |
| `parallel` | Case-insensitive class-code filter; all-class assignments remain included |

Each value is limited to 100 characters. `today` uses WIB (UTC+7). `week` includes
today through seven days ahead. Filters narrow the owner's results; they do not
switch identity or expand the underlying course-settings scope.

```sh
curl -H "Authorization: Bearer $MARBOT_API_KEY" \
  'https://<your-bot-host>/api/v1/assignments?timeframe=week&course=algoritme'
```

The success envelope is `{"success":true,"data":[...]}`. Entries use the existing
assignment representation, including course, title/description, deadline,
parallel codes, and completion metadata. An empty result is `data: []`.

## GET /attendance

First enable the feature in a private chat using `#remind-absen <your-NIM>`.
This endpoint reads the saved baseline; it **never triggers a university API
request, subscribes somebody, or sends a reminder**. It accepts **no query
parameters**, including NIM, `MahasiswaId`, and `userId`.

```sh
curl -H "Authorization: Bearer $MARBOT_API_KEY" \
  'https://<your-bot-host>/api/v1/attendance'
```

Example with synthetic data:

```json
{
  "success": true,
  "data": {
    "enabled": true,
    "nim": "G6401260001",
    "last_checked_at": "2026-11-09T10:00:00Z",
    "classes": [
      {"course": "Test Course", "class": "K1", "missed": 2, "meetings": 10}
    ]
  }
}
```

`last_checked_at` is UTC. Polling targets every 15 minutes during 08:00–12:00 and 13:00–17:00 WIB,
and hourly outside those windows; the data can lag
behind IPB Mobile. Missing classes and empty replies preserve older counts, so
some class data may predate the last successful check. Failed checks leave the
baseline and timestamp unchanged.

Polling slots are staggered per NIM and persisted across restarts. A scheduler
tick every 30 seconds checks only due records; it does not poll everyone every
30 seconds. Different NIMs are fetched serially with at least a two-second gap,
and shared-NIM subscriptions reuse a single response per batch. A database lock
prevents concurrent scheduled polling batches. Failed requests keep their
baseline and advance to the next slot. Overload/outages can delay checks.
Reminder messages omit counter values; the API still exposes `missed`/`meetings`.

Without a subscription, the response is still HTTP 200:

```json
{"success":true,"data":{"enabled":false,"nim":null,"last_checked_at":null,"classes":[]}}
```

`missed` mirrors `JumlahTidakHadir`: it includes both recorded absences and
unrecorded attendance. It does not establish physical absence or forgetting to
submit. First enrollment and new classes are silent; increases trigger one
private reminder, corrections lower the baseline, and unchanged counts do not
repeat messages. Delivery gets at most one attempt, so an uncertain/failed send
can lose a reminder. `#remind-absen off` deletes your subscription and stored
reminders; already claimed sends may complete.

## Rate limits, caching, and errors

The API allows **60 requests per 60-second fixed window per authenticated user
per bot process**, shared across endpoints and all their keys. Multiple bot
replicas have independent counters. Authentication happens before this limit.
WhatsApp commands also retain the bot's existing 5-commands/30-seconds limit.

Authenticated responses include `X-RateLimit-Limit`, `X-RateLimit-Remaining`, and
`X-RateLimit-Reset` (seconds until reset, not an epoch timestamp). HTTP 429 also
includes `Retry-After`; wait that many seconds before trying again.

Authenticated responses use `Cache-Control: no-store`. Do not put responses in a
shared cache or log their private contents. Attendance responses vary by
Authorization.

| HTTP | Meaning |
|---|---|
| 200 | Successful request, including empty data or disabled attendance |
| 400 | Oversized assignment filter or any attendance query parameter |
| 401 | Missing, invalid, or deleted key |
| 404 | Unknown route |
| 405 | Unsupported HTTP method |
| 429 | User's rate limit exceeded |
| 500 | Database/request handler failure |

Handler errors use `{"success":false,"data":null,"error":"..."}` for
attendance; assignment errors use `data: []`. Authentication/router errors can
have empty bodies; rate-limit errors return text. Check HTTP status before
assuming every response is JSON. No upstream credential or raw database error
is returned by the attendance handler.
