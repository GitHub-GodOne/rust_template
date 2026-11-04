/**
 * Bridge between the API client and the password-confirmation dialog: the
 * client asks, the mounted `ReauthHost` answers. Concurrent requests share
 * one dialog.
 */
type Opener = () => Promise<boolean>;

let opener: Opener | null = null;
let pending: Promise<boolean> | null = null;

export function registerReauthOpener(next: Opener | null) {
  opener = next;
}

/** Resolves true once the password was confirmed, false when cancelled. */
export function askForReauth(): Promise<boolean> {
  if (!opener) return Promise.resolve(false);
  if (!pending) {
    pending = opener().finally(() => {
      pending = null;
    });
  }
  return pending;
}
