declare module "@novnc/novnc" {
  type Credentials = { username?: string; password?: string; target?: string };
  export default class RFB extends EventTarget {
    constructor(
      target: HTMLElement,
      url: string,
      options?: { credentials?: Credentials },
    );
    scaleViewport: boolean;
    resizeSession: boolean;
    showDotCursor: boolean;
    background: string;
    disconnect(): void;
    sendCredentials(credentials: Credentials): void;
  }
}
