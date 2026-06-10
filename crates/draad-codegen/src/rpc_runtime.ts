/** Returned by `Rpc.listen`, call to unsubscribe. */
export type UnlistenFn = () => void;

/** HTTP verb a generated method maps to. */
export type HttpVerb = "GET" | "POST" | "PUT" | "PATCH" | "DELETE";

/**
 * Transport contract the generated API classes call into. Implement this
 * in your app (using fetch / axios / tauri / etc.) and pass it to
 * `new Api(rpc)`. Or use `defaultRpc(...)` below.
 */
export interface Rpc {
	/**
	 * Send an RPC request. The wire command is `{namespace}/{method}`.
	 * Verb defaults to POST (body-bearing); GET/DELETE callers must pass
	 * the verb explicitly so `args` is serialised as a query string.
	 */
	call<T>(command: string, args?: Record<string, unknown>, method?: HttpVerb): Promise<T>;

	/** Subscribe to a backend event. Returns an unsubscribe handle. */
	listen<T>(topic: string, handler: (payload: T) => void): UnlistenFn;

	/**
	 * Subscribe to every failed RPC call. The handler fires *before* the
	 * promise rejects, so local `try { ... } catch (e)` paths still see
	 * the error too. Useful for global concerns: toast on error, route
	 * 401s to a sign-in flow, ship a Sentry breadcrumb, etc. Optional on
	 * the interface — custom transports can omit it.
	 */
	onError?(handler: (err: RpcError) => void): UnlistenFn;
}

/**
 * Thrown (or returned) by `Rpc.call` implementations on failure.
 *
 * The generic `E` is the shape of the server's error body — generated
 * methods tag their `@throws` with the concrete error type pulled from
 * the trait's `Result<_, E>` return.
 */
export class RpcError<E = unknown> extends Error {
	constructor(
		public readonly code: string,
		/** HTTP status code from the response (or 0 if the request never reached the server). */
		public readonly status: number,
		/**
		 * Parsed JSON body from the server, when present. Falls back to
		 * the raw response text on non-JSON responses, and to `null`
		 * when the body was empty.
		 */
		public readonly body: E | string | null,
		message: string,
	) {
		super(message);
		this.name = "RpcError";
	}
}

export type DefaultRpcOptions = {
	/** Base URL for RPC calls. Requests go to `${baseUrl}/${command}`. */
	baseUrl?: string;
	/** WebSocket URL for events. Omit to make `listen()` throw. */
	wsUrl?: string;
	/** Extra headers added to every RPC call. */
	headers?: Record<string, string>;
	/** Disable WebSocket auto-reconnect. Default: true. */
	reconnect?: boolean;
	/** Cap on the backoff between reconnect attempts. Default: 30000. */
	maxReconnectDelayMs?: number;
};

/**
 * Default `Rpc` implementation: POST JSON for calls, a shared WebSocket
 * for events. Wire format:
 *
 *  - Calls: `POST {baseUrl}/{command}` with body `args`. 2xx responses
 *    are parsed as JSON and returned; non-2xx are thrown as `RpcError`.
 *  - Events: WS frames shaped `{ topic: string, payload: T }`.
 *
 * Auto-reconnects with exponential backoff + jitter (capped at
 * `maxReconnectDelayMs`) as long as there are active subscribers. Replace
 * with your own `Rpc` for auth, custom transports, or a different wire
 * format.
 */
export function defaultRpc(opts: DefaultRpcOptions = {}): Rpc {
	const baseUrl = opts.baseUrl ?? "/api";
	const maxDelayMs = opts.maxReconnectDelayMs ?? 30000;
	const shouldReconnect = opts.reconnect !== false;
	const subs = new Map<string, Set<(payload: unknown) => void>>();
	const errorHandlers = new Set<(err: RpcError) => void>();
	let ws: WebSocket | undefined;
	let reconnectDelayMs = 500;
	let reconnectTimer: ReturnType<typeof setTimeout> | undefined;

	function fireError(err: RpcError): RpcError {
		// Notify every observer; a single throwing handler must not
		// take out the others, hence the per-handler try/catch.
		for (const h of errorHandlers) {
			try { h(err); } catch { /* swallow handler errors */ }
		}
		return err;
	}

	function openWs(url: string): WebSocket {
		const sock = new WebSocket(url);
		sock.addEventListener("open", () => { reconnectDelayMs = 500; });
		sock.addEventListener("message", (ev) => {
			try {
				const { topic, payload } = JSON.parse(String(ev.data));
				subs.get(topic)?.forEach((h) => h(payload));
			} catch {
				/* ignore malformed frames */
			}
		});
		sock.addEventListener("close", () => {
			ws = undefined;
			if (!shouldReconnect || subs.size === 0) return;
			const base = Math.min(reconnectDelayMs, maxDelayMs);
			const jitter = base * (0.8 + Math.random() * 0.4);
			reconnectDelayMs = Math.min(reconnectDelayMs * 2, maxDelayMs);
			reconnectTimer = setTimeout(() => {
				reconnectTimer = undefined;
				if (subs.size > 0 && !ws) ws = openWs(url);
			}, jitter);
		});
		return sock;
	}

	function ensureWs(): WebSocket {
		if (!opts.wsUrl) {
			throw new RpcError("NO_WS_URL", 0, null, "defaultRpc: wsUrl not configured; cannot listen()");
		}
		if (ws && ws.readyState !== WebSocket.CLOSED) return ws;
		ws = openWs(opts.wsUrl);
		return ws;
	}

	return {
		async call<T>(command: string, args?: Record<string, unknown>, method: HttpVerb = "POST"): Promise<T> {
			const hasBody = method === "POST" || method === "PUT" || method === "PATCH";
			let url = `${baseUrl}/${command}`;
			const init: RequestInit = { method, headers: { ...opts.headers } };
			if (hasBody) {
				(init.headers as Record<string, string>)["Content-Type"] = "application/json";
				init.body = JSON.stringify(args ?? {});
			} else if (args) {
				const params = new URLSearchParams();
				for (const [k, v] of Object.entries(args)) {
					if (v === undefined || v === null) continue;
					if (Array.isArray(v)) {
						for (const item of v) params.append(k, String(item));
					} else {
						params.append(k, String(v));
					}
				}
				const qs = params.toString();
				if (qs) url += `?${qs}`;
			}
			// Every failure path constructs an `RpcError`, fires the
			// global `onError` observers, then throws — so callers'
			// local try/catch and global handlers both see it.
			let res: Response;
			try {
				res = await fetch(url, init);
			} catch (e) {
				const msg = e instanceof Error ? e.message : String(e);
				throw fireError(new RpcError("NETWORK", 0, null, msg));
			}
			if (!res.ok) {
				// Best effort at giving callers the typed body their
				// `@throws` jsdoc promised: read once as text, try
				// JSON.parse, fall back to the raw string. Empty body
				// becomes null so `instanceof` checks stay clean.
				const raw = await res.text();
				let body: unknown = null;
				if (raw.length > 0) {
					try {
						body = JSON.parse(raw);
					} catch {
						body = raw;
					}
				}
				const message = typeof body === "string" ? body : res.statusText;
				throw fireError(new RpcError(`HTTP_${res.status}`, res.status, body as never, message));
			}
			try {
				return (await res.json()) as T;
			} catch (e) {
				const msg = e instanceof Error ? e.message : String(e);
				throw fireError(new RpcError("BAD_BODY", res.status, null, msg));
			}
		},
		listen<T>(topic: string, handler: (payload: T) => void): UnlistenFn {
			ensureWs();
			let set = subs.get(topic);
			if (!set) {
				set = new Set();
				subs.set(topic, set);
			}
			const wrapped = handler as (p: unknown) => void;
			set.add(wrapped);
			return () => {
				set!.delete(wrapped);
				if (set!.size === 0) subs.delete(topic);
				if (subs.size === 0 && reconnectTimer) {
					clearTimeout(reconnectTimer);
					reconnectTimer = undefined;
				}
			};
		},
		onError(handler: (err: RpcError) => void): UnlistenFn {
			errorHandlers.add(handler);
			return () => { errorHandlers.delete(handler); };
		},
	};
}
