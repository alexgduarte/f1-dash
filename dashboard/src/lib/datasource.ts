import type { MessageInitial, MessageUpdate } from "@/types/message.type";
import type { Round } from "@/types/schedule.type";
import type { ArchiveMeeting } from "@/types/replay.type";
import type { ReplayRequest } from "@/stores/useReplayStore";
import type { SeriesId } from "@/lib/series";

import { env } from "@/env";
import { isNative } from "@/lib/platform";

// Where live and schedule data come from:
// - web: the realtime service over server-sent events and the api service over HTTP
// - native app: the feed adapters compiled into the app, over Tauri IPC, so the
//   app connects to the timing feeds directly without any f1-dash server

export type FeedHandlers = {
	onInitial: (data: MessageInitial) => void;
	onUpdate: (data: MessageUpdate) => void;
	onConnection: (connected: boolean) => void;
};

type NativeFeedMessage = { kind: "initial" | "update"; data: string } | { kind: "status"; data: boolean };

const listenToEventSource = (url: string, handlers: FeedHandlers): (() => void) => {
	const sse = new EventSource(url);

	// connected means the server reached the timing feed, not just that this stream is open
	sse.onerror = () => handlers.onConnection(false);

	sse.addEventListener("initial", (message) => handlers.onInitial(JSON.parse(message.data)));
	sse.addEventListener("update", (message) => handlers.onUpdate(JSON.parse(message.data)));
	sse.addEventListener("status", (message) => handlers.onConnection(message.data === "true"));

	return () => sse.close();
};

const liveUrl = (): string | null => {
	if (!env.NEXT_PUBLIC_LIVE_URL) {
		console.error("NEXT_PUBLIC_LIVE_URL is not set, cannot connect to the realtime service");
		return null;
	}

	return env.NEXT_PUBLIC_LIVE_URL;
};

/** Subscribes through a native command that streams into a channel. */
const listenToChannel = (command: string, args: Record<string, unknown>, handlers: FeedHandlers): (() => void) => {
	let closed = false;
	let unsubscribe: (() => void) | null = null;

	(async () => {
		const { Channel, invoke } = await import("@tauri-apps/api/core");

		const channel = new Channel<NativeFeedMessage>();

		channel.onmessage = (message) => {
			if (closed) return;

			switch (message.kind) {
				case "initial":
					handlers.onInitial(JSON.parse(message.data));
					break;
				case "update":
					handlers.onUpdate(JSON.parse(message.data));
					break;
				case "status":
					handlers.onConnection(message.data);
					break;
			}
		};

		try {
			const id = await invoke<number>(command, { ...args, channel });
			const stop = () => void invoke("feed_unsubscribe", { id });

			if (closed) stop();
			else unsubscribe = stop;
		} catch (error) {
			console.error(`${command} failed`, error);
			handlers.onConnection(false);
		}
	})();

	return () => {
		closed = true;
		unsubscribe?.();
	};
};

/** Subscribes to a series' live feed. Returns an unsubscribe function. */
export const subscribeFeed = (series: SeriesId, handlers: FeedHandlers): (() => void) => {
	if (isNative()) return listenToChannel("feed_subscribe", { series }, handlers);

	const base = liveUrl();
	if (!base) {
		handlers.onConnection(false);
		return () => {};
	}

	return listenToEventSource(`${base}/api/realtime?series=${series}`, handlers);
};

/** Plays back a past session from the archive. Returns an unsubscribe function. */
export const subscribeReplay = (path: string, request: ReplayRequest, handlers: FeedHandlers): (() => void) => {
	const full = { path, from: request.from, speed: request.speed, paused: request.paused };

	if (isNative()) return listenToChannel("replay_subscribe", { request: full }, handlers);

	const base = liveUrl();
	if (!base) {
		handlers.onConnection(false);
		return () => {};
	}

	// EventSource reconnects to the same URL by itself, which would restart the
	// replay where it began; reconnect by hand from the last known position
	let position = request.from;
	let closed = false;
	let retry: ReturnType<typeof setTimeout> | null = null;
	let close: () => void = () => {};

	const tracking: FeedHandlers = {
		...handlers,
		onInitial: (data) => {
			if (data.Replay?.Position !== undefined) position = data.Replay.Position;
			handlers.onInitial(data);
		},
		onUpdate: (data) => {
			if (data.Replay?.Position !== undefined) position = data.Replay.Position;
			handlers.onUpdate(data);
		},
	};

	const open = () => {
		const params = new URLSearchParams({ path, speed: String(request.speed), paused: String(request.paused) });
		if (position !== undefined) params.set("from", String(Math.round(position)));

		const sse = new EventSource(`${base}/api/replay?${params}`);

		sse.addEventListener("initial", (message) => tracking.onInitial(JSON.parse(message.data)));
		sse.addEventListener("update", (message) => tracking.onUpdate(JSON.parse(message.data)));
		sse.addEventListener("status", (message) => handlers.onConnection(message.data === "true"));

		sse.onerror = () => {
			sse.close();
			handlers.onConnection(false);
			if (!closed) retry = setTimeout(open, 2000);
		};

		close = () => sse.close();
	};

	open();

	return () => {
		closed = true;
		if (retry) clearTimeout(retry);
		close();
	};
};

const fetchApi = async <T>(path: string): Promise<T | null> => {
	if (!env.NEXT_PUBLIC_API_URL) {
		console.error("NEXT_PUBLIC_API_URL is not set, cannot reach the api service");
		return null;
	}

	const res = await fetch(`${env.NEXT_PUBLIC_API_URL}${path}`, { cache: "no-store" });

	if (res.status === 204) return null;
	if (!res.ok) throw new Error(`api responded with ${res.status}`);

	return res.json();
};

const invokeNative = async <T>(command: string, args: Record<string, unknown>): Promise<T> => {
	const { invoke } = await import("@tauri-apps/api/core");
	return invoke<T>(command, args);
};

export const fetchReplaySessions = async (year: number): Promise<ArchiveMeeting[]> => {
	if (isNative()) return invokeNative<ArchiveMeeting[]>("replay_sessions", { year });

	const base = liveUrl();
	if (!base) throw new Error("NEXT_PUBLIC_LIVE_URL is not set");

	const res = await fetch(`${base}/api/replay/sessions?year=${year}`, { cache: "no-store" });
	if (!res.ok) throw new Error(`replay sessions responded with ${res.status}`);

	return res.json();
};

export const fetchSchedule = async (series: SeriesId): Promise<Round[] | null> =>
	isNative() ? invokeNative<Round[]>("schedule", { series }) : fetchApi<Round[]>(`/api/schedule?series=${series}`);
