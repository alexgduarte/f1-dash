import type { MessageInitial, MessageUpdate } from "@/types/message.type";
import type { Round } from "@/types/schedule.type";
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

const subscribeWeb = (series: SeriesId, handlers: FeedHandlers): (() => void) => {
	if (!env.NEXT_PUBLIC_LIVE_URL) {
		console.error("NEXT_PUBLIC_LIVE_URL is not set, cannot connect to the realtime service");
		handlers.onConnection(false);
		return () => {};
	}

	const sse = new EventSource(`${env.NEXT_PUBLIC_LIVE_URL}/api/realtime?series=${series}`);

	// connected means the server reached the timing feed, not just that this stream is open
	sse.onerror = () => handlers.onConnection(false);

	sse.addEventListener("initial", (message) => handlers.onInitial(JSON.parse(message.data)));
	sse.addEventListener("update", (message) => handlers.onUpdate(JSON.parse(message.data)));
	sse.addEventListener("status", (message) => handlers.onConnection(message.data === "true"));

	return () => sse.close();
};

const subscribeNative = (series: SeriesId, handlers: FeedHandlers): (() => void) => {
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
			const id = await invoke<number>("feed_subscribe", { series, channel });
			const stop = () => void invoke("feed_unsubscribe", { id });

			if (closed) stop();
			else unsubscribe = stop;
		} catch (error) {
			console.error("failed to subscribe to native feed", error);
			handlers.onConnection(false);
		}
	})();

	return () => {
		closed = true;
		unsubscribe?.();
	};
};

/** Subscribes to a series' live feed. Returns an unsubscribe function. */
export const subscribeFeed = (series: SeriesId, handlers: FeedHandlers): (() => void) =>
	isNative() ? subscribeNative(series, handlers) : subscribeWeb(series, handlers);

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

export const fetchSchedule = async (series: SeriesId): Promise<Round[] | null> =>
	isNative() ? invokeNative<Round[]>("schedule", { series }) : fetchApi<Round[]>(`/api/schedule?series=${series}`);
