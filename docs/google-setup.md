# Connecting Kith to Google Contacts

Kith talks to Google through **your own** Google Cloud project, so nobody else sees your contacts and no shared
quota is involved. Setting it up takes about ten minutes and is done once.

You will end up with two strings, a **client ID** and a **client secret**, which you paste into Kith's Google settings.

> Google renames menus now and then. The names below are the current ones (the "Google Auth Platform" pages); the older
> "OAuth consent screen" pages are mentioned where they differ.

## 1. Create a project

1. Open <https://console.cloud.google.com/> and sign in with the Google account whose contacts you want to sync.
2. Accept the terms if asked.
3. Click the **project picker** at the top left (it shows the current project name or "Select a project"), then **New project**.
4. Name it, for example `Kith`. Leave "Organization" and "Location" as they are. Click **Create**.
5. Make sure the new project is selected in the picker.

## 2. Enable the People API

1. Open the navigation menu (the three-line button) and choose **APIs & Services** > **Library**.
2. Search for `People API` and open **Google People API**.
3. Click **Enable**.

Without this step Kith reports "People API has not been used in project … or it is disabled".

## 3. Configure the OAuth consent screen

1. **APIs & Services** > **OAuth consent screen**. In the new layout this opens **Google Auth Platform**; click **Get started**.
2. **App information**: App name `Kith`, User support email = your address. Click **Next**.
3. **Audience**: choose **External**. Click **Next**. (If you use a Google Workspace account you may also see "Internal"; that works only inside your organization.)
4. **Contact information**: your e-mail address. Click **Next**.
5. Tick the agreement to the "Google API Services User Data Policy" and click **Continue**, then **Create**.
6. Open **Data Access** (older layout: step "Scopes" > **Add or remove scopes**) and click **Add or remove scopes**.
   Find `.../auth/contacts` (description: "See, edit, download, and permanently delete your contacts"), tick it, click **Update**, then **Save**.
   Kith also uses `openid` and `email` to show which account is connected; they need no approval and may already be listed.

## 4. Publish the app ("In production")

Open **Audience** (older layout: **OAuth consent screen**) and look at **Publishing status**.

* A new project starts in **Testing**. In Testing, Google **expires the refresh token after 7 days**, so Kith would ask you to sign in again every week. (In Testing you must also list yourself under **Test users** > **Add users**.)
* For personal use, click **Publish app** and confirm. The status becomes **In production**. No review is needed for an app that only you use: Google simply shows an "unverified app" notice.

The contacts scope is a **sensitive scope**. An unverified app in production is limited to 100 users, which is irrelevant here, and
the first time you sign in Google shows **"Google hasn't verified this app"**. That is expected, because the app is yours. Click **Advanced**, then
**Go to Kith (unsafe)**, and on the next page keep the contacts permission ticked and click **Continue**. You see this once.

## 5. Create the OAuth client

1. **APIs & Services** > **Credentials**, or in Google Auth Platform: **Clients**.
2. Click **Create client** (older layout: **Create credentials** > **OAuth client ID**).
3. **Application type**: choose **Desktop app**. This matters: the other types need fixed redirect addresses and will not work with Kith.
4. Name: `Kith desktop`. Click **Create**.
5. A dialog shows **Client ID** (ends in `.apps.googleusercontent.com`) and **Client secret**. Copy both; you can show them again any time from the client's page.

## 6. Paste them into Kith

1. In Kith click the **gear** at the bottom left (Settings) and scroll to **Google Contacts**.
2. Paste the **Client ID** and the **Client secret** and click **Save**.
3. Click **Connect Google account**. Your browser opens Google's page; choose your account, confirm the "unverified app" notice (step 4), keep the **contacts** permission ticked and continue. The browser tab then says Kith is connected and you can close it.
4. Click **Preview what a sync would do** to see the plan, then **Sync now**. This first sync only **reads** from Google (the switch "Send my changes to Google" is off). Look through your contacts; when you are happy, switch it on.

## Good to know

* **The client secret of a desktop app is not really secret.** Google's own documentation says so: desktop apps cannot hide it, and the real protection is that sign-in needs your Google password and your consent (Kith also uses PKCE). Still, do not publish it. Kith writes its settings file (`~/.config/kith/settings.json`) with mode `0600`, readable only by you.
* **Where the sign-in is kept.** The long-lived refresh token goes into your desktop's keyring (Secret Service: GNOME Keyring, KWallet, KeePassXC…). If none is available it is written to `~/.config/kith/google-token.json` with mode `0600`.
* **"Sign in again" every week?** Your project is still in **Testing**; go back to step 4. If you were signed in while in Testing, sign in once more after publishing.
* **"Access blocked" or "Error 403: access_denied".** In Testing mode your account must be a test user (step 4), or publish the app.
* **Revoking access.** Sign out in Kith, or remove Kith at <https://myaccount.google.com/permissions>. Deleting the Cloud project also ends it.
* **Quotas.** Google allows roughly one write per second per user. A first upload of thousands of contacts therefore takes a while; Kith paces itself and retries when Google asks it to slow down.
* **Safety net.** Kith writes a backup of your whole address book before the first change of every sync, and moves contacts deleted on Google to its own trash. Contacts deleted in Kith are only deleted on Google if you switch that on. Anything deleted on Google stays in Google's trash for 30 days (<https://contacts.google.com/> > **Trash**).
