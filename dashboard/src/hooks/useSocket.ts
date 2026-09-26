import { useEffect, useRef, useState } from "react";

import type { MessageInitial, MessageUpdate } from "@/types/message.type";
import type { ReplayRequest, ReplaySession } from "@/stores/useReplayStore";
import type { SeriesId } from "@/lib/series";

import { subscribeFeed, subscribeReplay } from "@/lib/datasource";

type Props = {
	series: SeriesId;
	replay: { session: ReplaySession; request: ReplayRequest } | null;
	handleInitial: (data: MessageInitial) => void;
	handleUpdate: (data: MessageUpdate) => void;
	reset: () => void;
};

export const useSocket = ({ series, replay, handleInitial, handleUpdate, reset }: Props) => {
	const [connected, setConnected] = useState<boolean>(false);

	// what is being watched; a replay restarting at another position is still
	// the same session, so its data stays on screen while it reconnects
	const target = replay ? `replay:${replay.session.path}` : `live:${series}`;
	const targetRef = useRef<string | null>(null);

	const path = replay?.session.path;
	const from = replay?.request.from;
	const speed = replay?.request.speed;
	const paused = replay?.request.paused;

	useEffect(() => {
		if (targetRef.current !== target) {
			targetRef.current = target;
			reset();
		}

		const handlers = {
			onInitial: handleInitial,
			onUpdate: handleUpdate,
			onConnection: setConnected,
		};

		const unsubscribe =
			path !== undefined
				? subscribeReplay(path, { from, speed: speed ?? 1, paused: paused ?? false }, handlers)
				: subscribeFeed(series, handlers);

		return () => {
			unsubscribe();
			setConnected(false);
		};
		// the handlers are recreated every render but only read refs and stable setters
		// eslint-disable-next-line react-hooks/exhaustive-deps
	}, [target, series, path, from, speed, paused]);

	return { connected };
};
