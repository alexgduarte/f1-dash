import { useEffect, useState } from "react";

import type { MessageInitial, MessageUpdate } from "@/types/message.type";
import type { SeriesId } from "@/lib/series";

import { subscribeFeed } from "@/lib/datasource";

type Props = {
	series: SeriesId;
	handleInitial: (data: MessageInitial) => void;
	handleUpdate: (data: MessageUpdate) => void;
	reset: () => void;
};

export const useSocket = ({ series, handleInitial, handleUpdate, reset }: Props) => {
	const [connected, setConnected] = useState<boolean>(false);

	useEffect(() => {
		reset();

		const unsubscribe = subscribeFeed(series, {
			onInitial: handleInitial,
			onUpdate: handleUpdate,
			onConnection: setConnected,
		});

		return () => {
			unsubscribe();
			setConnected(false);
		};
		// the handlers are recreated every render but only read refs and stable setters
		// eslint-disable-next-line react-hooks/exhaustive-deps
	}, [series]);

	return { connected };
};
