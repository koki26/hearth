# Email on your own Fireplace

Email is **optional**. A Fireplace with `backend = "none"`, the default,
works completely: players reset a forgotten password with one of the
recovery codes they were given, or you run `fireplace admin reset-password
<handle>` and hand them the link. Set up email only if you want players to be
able to reset by themselves.

What it sends, and nothing else (`docs/decisions.md` D63):

* a link to confirm an address a player added,
* a reset link the player asked for,
* a notice when the password changes, or when the address changes (sent to
  the *old* address).

Plain text, in English, a few lines each.

## The settings

In `fireplace.toml`:

```toml
[mail]
backend = "smtp"                              # "none" | "smtp" | "log"
public_url = "https://fireplace.example.org"  # links in emails start with this
from = "Hearth <hearth@example.org>"
reply_to = ""                                 # optional
provider = ""                                 # who runs the SMTP server, if not you

[mail.smtp]
host = "mail.example.org"
port = 587
security = "starttls"    # "starttls" (587) | "tls" (465) | "none" (localhost only)
username = "hearth@example.org"
password_file = "/opt/fireplace/secrets/smtp-password"
timeout_secs = 10

[operator]
name = "Your name or organisation"   # the data controller, on /privacy
contact_email = "privacy@example.org"
hosting = "Hetzner, Falkenstein, Germany (EU)"
authority = "the data protection authority of your country, with a link"
min_age = 16          # some EU countries allow lower, e.g. 13-15
backup_days = 14
```

| Key | |
|---|---|
| `backend` | `none` sends nothing and hides every email field. `smtp` sends. `log` prints whole emails, links included, to the log: for development, and refused in a release build unless `FIREPLACE_DEV_MAIL=1`. |
| `public_url` | Where players reach this Fireplace. Must be `https://`, except `http://localhost`. |
| `from`, `reply_to` | Mailboxes, `Name <address>` or a bare address. Use an address on a domain you control (see DNS below). |
| `provider` | If a company runs your SMTP server, name it here. It is your **processor** under GDPR, and `/privacy` says so. |
| `security` | `starttls` on 587, `tls` on 465. The Fireplace refuses `tls` on 587 and `starttls` on 465, which are almost always a mix-up, and refuses `none` to anything but `localhost`. |
| `username` | Empty means no login (a local relay). |
| `[operator]` | **Required when email is on.** The server will not start with `name` or `contact_email` empty, because `/privacy` must say who is responsible. |

All of it is checked at startup. Anything wrong stops the server with a
sentence naming the key. Nothing is half-working.

## The password

Not in `fireplace.toml`. A file with the password on one line, readable by
the service user alone:

```sh
sudo install -d -m 700 -o hearth -g hearth /opt/fireplace/secrets
sudo install -m 600 -o hearth -g hearth /dev/null /opt/fireplace/secrets/smtp-password
sudo -u hearth nano /opt/fireplace/secrets/smtp-password
```

Or the environment variable `FIREPLACE_SMTP_PASSWORD`, which wins over the
file. With systemd, put it in a root-only `EnvironmentFile=` rather than in
the unit. The Fireplace never prints it, and nor do its errors.

Use an account that exists only for this, so its password is the only thing
lost if this box is.

## Port 25 is not for you

Many clouds (Oracle, AWS, Google, Azure, most VPS hosts) **block outgoing
port 25** to stop spam, so do not try to deliver straight to recipients.
Send through a real mail server you have an account on, on **587**
(STARTTLS) or **465** (TLS). That is what `[mail.smtp]` is for.

## DNS, so letters arrive

Without these, reset links land in spam or nowhere. They go on the domain in
`from`, set at whoever hosts your mail. Your mail provider will tell you the
exact values.

| Record | What it says | Example |
|---|---|---|
| **SPF** (`TXT` on the domain) | which servers may send for it | `v=spf1 include:_spf.yourprovider.example -all` |
| **DKIM** (`TXT` on `<selector>._domainkey`) | the public key the mail server signs with | given by your provider, or by your own server's DKIM setup |
| **DMARC** (`TXT` on `_dmarc`) | what to do when SPF/DKIM fail, and where to send reports | `v=DMARC1; p=quarantine; rua=mailto:dmarc@example.org` |

Start DMARC at `p=none` while you check the reports, then tighten it. If you
run the mail server yourself, also give its IP a **reverse DNS (PTR)** name
that matches its `HELO`.

## Testing it

```sh
cd /opt/fireplace
sudo -u hearth ./fireplace admin test-mail you@example.org fireplace.toml
```

It checks the whole config as the server would, sends one test email, and
prints the SMTP server's answer (`sent: 250 …`), or the whole error: wrong
password, refused TLS, a timeout. Then look at the delivered email's headers:
`spf=pass`, `dkim=pass` and `dmarc=pass` are what you want.

Then restart the Fireplace. Its first log line says `email=true`, and
`GET /features` says `"email": true`, which is what makes the game show
email fields.

## When sending fails

The player is never told: the answer to "reset my password" is the same
whether a letter went out or not, which is what keeps it from saying who has
an account. The Fireplace tries three times, over about a minute, and logs
each failure with the kind of letter and the SMTP code, **never the
address** (`journalctl -u fireplace | grep "could not send"`).

## Privacy

Turning email on means you now hold email addresses. Read
[PRIVACY-OPS.md](PRIVACY-OPS.md): encrypted backups, what to do on a breach,
and how to answer a request to see or delete someone's data.
