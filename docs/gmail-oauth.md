# Gmail sign-in with OAuth2

Rusty Inbox can sign in to Gmail with Google's OAuth2 instead of an app password. It uses the
installed-app flow: PKCE (S256), a loopback redirect to `http://127.0.0.1:<random port>`, the
system browser, and the XOAUTH2 login for IMAP and SMTP. Access tokens are renewed automatically
from the refresh token; the refresh token is stored in the same local secret store as passwords
(`secrets.sqlite`, mode 0600) under `oauth:<email>`. Removing the account deletes it.

Without a Google client ID the button is disabled with an explanation, and the app password flow
keeps working.

## What the maintainer must do (one time)

1. Open the [Google Cloud console](https://console.cloud.google.com/) and create a project
   (for example "Rusty Inbox").
2. Enable the **Gmail API** (APIs & Services → Library). IMAP and SMTP access with the
   `https://mail.google.com/` scope is covered by the same consent screen.
3. APIs & Services → **OAuth consent screen**: user type *External*, app name "Rusty Inbox",
   support email, the privacy policy / home page link, and add the scope
   `https://mail.google.com/`.
4. APIs & Services → **Credentials** → *Create credentials* → *OAuth client ID* → application
   type **Desktop app**. Copy the client ID and the client secret. Google issues a secret for
   desktop apps, but it is not confidential: it is allowed to ship inside the app.
5. Give the client to the build (never commit it):

   ```sh
   RUSTY_INBOX_GOOGLE_CLIENT_ID=123456-abc.apps.googleusercontent.com \
   RUSTY_INBOX_GOOGLE_CLIENT_SECRET=GOCSPX-... \
   cargo build --release
   ```

   For the official releases, add both as repository secrets and pass them as environment
   variables to the build steps in `.github/workflows/release.yml`.
6. **Verification limits.** `https://mail.google.com/` is a *restricted* scope. Until the app is
   verified by Google (including a security assessment), an unverified app in *Testing* or
   *In production* status is limited to **100 users**, shows an "unverified app" warning, and in
   *Testing* status refresh tokens expire after 7 days. Add testers under *OAuth consent screen →
   Test users* while you are in testing.

## Using your own client ID

Anyone can bring their own client: *Settings → Accounts → Add account → Gmail → Use my own Google
client ID*. The values are saved as the `googleClientId` and `googleClientSecret` settings.

Order of precedence: the user's setting, then the environment variables
`RUSTY_INBOX_GOOGLE_CLIENT_ID` / `RUSTY_INBOX_GOOGLE_CLIENT_SECRET` at run time, then the values
compiled in. Refresh tokens belong to the client that issued them: after changing the client ID,
remove the Gmail account and add it again.
