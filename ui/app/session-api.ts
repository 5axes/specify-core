/** The specify-core session API spoken by this app, independent of its release version. */
export const SESSION_API = 1;

export class SessionApiMismatch extends Error {
  constructor(api: number) {
    super(`This Specify page speaks session API ${SESSION_API}, but specify-core speaks ${api}: update specify-core or the app`);
    this.name = 'SessionApiMismatch';
  }
}

/** specify-core 0.2.0 omits api; an explicitly different version is incompatible. */
export function requireSessionApi(api: number | undefined): void {
  if (api !== undefined && api !== SESSION_API) throw new SessionApiMismatch(api);
}
