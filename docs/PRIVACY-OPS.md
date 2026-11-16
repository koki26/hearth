# Running a Fireplace under GDPR

For whoever runs a Fireplace. This is a working checklist, not legal advice. If your
Fireplace grows past friends and family, have it read by somebody who does
this for a living.

The players' side of the same thing is `/privacy` on your server, filled in
from `[operator]` and `[mail]` in `fireplace.toml`. Keep the two in step.

## Record of processing (Art. 30)

Keep this, filled in, somewhere you can find it. A copy of this section with
your details is enough at this size.

| | |
|---|---|
| **Controller** | `[operator] name`, `contact_email` |
| **Purpose** | Player accounts for Hearth: signing in, friends, where friends are, getting back into an account |
| **People** | Players who make an account |
| **Data** | handle, display name, creation time; password and recovery codes as one-way hashes; friends and requests; location-sharing setting; optional email and whether it is confirmed. In memory only: which server a friend-visible player is on (2 minutes), attempt counters keyed by IP (up to an hour) |
| **Lawful basis** | Performance of the service, Art. 6(1)(b), for the account and its emails; legitimate interest, Art. 6(1)(f), for the attempt counters against password guessing |
| **Recipients** | Nobody. Game servers see a display name and a per-server number. The mail provider, if any, as a processor |
| **Transfers outside the EU** | None, if the server and the mail provider are in the EU. Check both |
| **Retention** | Until the account is deleted; unconfirmed email 7 days; backups `backup_days` (14) |
| **Security** | TLS in front (Caddy); argon2id passwords; signed tokens; data readable by the service user only; backups encrypted off the box; SMTP password in a 0600 file |

### The mail server: yours or somebody else's

Write down which:

* **Self-run** (your own Postfix or similar): no processor. You hold the
  mail logs, which contain addresses, so set how long they are kept
  (Postfix logs rotate through syslog: a week or two is plenty) and include
  that in this record.
* **Hosted by a company** (a mailbox provider, a transactional email
  service): that company is your **processor** (Art. 28). Put its name in
  `[mail] provider` so `/privacy` says so, and note here where its data
  processing agreement (DPA) or terms are, and where it keeps data:

  | | |
  |---|---|
  | Provider | |
  | DPA / terms (link, date accepted) | |
  | Where it stores data | |

For the official server (`mail.example.org`): fill this in with
whoever runs that mail host. If it is a hosting company's mail service, it
is a processor and its DPA is usually part of the hosting terms.

## A request to see or delete data

Requests arrive at `contact_email`. Answer within **one month** (Art. 12).

1. **Check it is them.** Reply asking them to send the request from the
   confirmed email on the account, or to do it themselves while signed in,
   which is the easier way and needs no check at all:
   * **access / portability**: the account page, *Download my data*
     (`GET /me/export`), is everything stored about them;
   * **erasure**: *Delete account* (`DELETE /me`) removes it at once, email
     included;
   * **rectification**: they change their display name or email themselves.
2. **If they cannot sign in** and want it deleted: confirm it is them (the
   confirmed address on the account, or a recovery code they can read to
   you), then get into the account yourself and delete it the normal way,
   so every friend list is cleaned up in the same save:

   ```sh
   cd /opt/fireplace && sudo -u hearth ./fireplace admin reset-password <handle>
   ```

   Open the link, set a throwaway password, sign in on the account page
   with it, and press *Delete account*.
3. **Backups.** Tell them the data rolls out of backups within
   `backup_days`. **If you ever restore a backup, delete again every account
   whose erasure was answered after that backup was taken.** Keep a short
   list (date, handle) of erasures for exactly this, and drop entries older
   than `backup_days`.
4. Reply saying what you did, and keep that email as your record.

An **objection** or **restriction** request is rare here, because nothing is
done to anybody beyond running their account. The honest answer is usually
to offer deletion.

## A data breach

A breach is anybody getting at the data who should not: the box
compromised, `people.json` or a backup leaked, the SMTP account taken over.

1. **Contain it.** Take the box off the internet if you must. Rotate the
   SMTP password. If `key.pk8` may have leaked, every token can be forged:
   generate a new key (move the old one aside and restart), which signs
   everybody out, and pull a fresh public key into every Flame.
2. **Write down** what happened, when you found out, what data, how many
   people, and what you did. You must keep this record whether or not you
   report (Art. 33(5)).
3. **Report to the authority within 72 hours** of finding out, unless the
   breach is unlikely to put anybody at risk (Art. 33). Leaked email
   addresses tied to accounts are usually reportable; a leak of password
   *hashes* is too, because argon2id slows guessing but does not stop it.
   Your country's data protection authority usually has a form for it.
   Report late rather than not at all, and say why it was late.
4. **Tell the players** if the risk to them is high (Art. 34): the accounts
   page, and an email to confirmed addresses (this is a security notice, which
   is what the address is for). Tell them to change the password, and the
   same password anywhere else they used it.

## Things that keep this small

* Logs never contain an email, password, token or reset link. There is a test
  for it (`no_address_password_or_token_ever_reaches_the_log`). Keep it
  passing.
* IP addresses are only in memory, for rate limiting, and gone on restart.
* Backups: encrypted on your own machine, the last `backup_days` only
  ([ORACLE.md](ORACLE.md), step 7).
* No analytics, no third-party fonts or scripts, no cookies. Keep it that
  way and you need no cookie banner and no consent records.

## Changing the terms or the privacy notice

The pages are built from `fireplace.toml` and the code, so a change ships
with a release. When it matters to people (new data, a new purpose, a new
processor, a shorter retention), tell the ones who asked:

1. Write the letter: first line the subject, then the body in plain words --
   what changed, from when, and that the full text is on `/terms` and
   `/privacy` (the email links both and says how to stop these emails).

   ```text
   The privacy notice changes on 1 March
   From 1 March, emails go through a new provider, Example Mail s.r.o. ...
   ```

2. Send it, from where `fireplace.toml` is:

   ```sh
   cd /opt/fireplace && sudo -u hearth ./fireplace admin policy-mail letter.txt
   ```

   It goes only to accounts that ticked *email me when the terms or privacy
   notice change* **and** have a confirmed address; it prints how many were
   sent. Nobody else is written to, and nothing else may be sent this way:
   no news, no marketing. That consent covers these notices and nothing more.
3. Send it **before** the change takes effect, and keep the letter as your
   record.

Under-age sign-ups never reach you: the Fireplace refuses an account below
`min_age` (`[operator]` in `fireplace.toml`) and keeps no birth date.
