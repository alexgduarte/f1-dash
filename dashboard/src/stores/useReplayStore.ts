import { create } from "zustand";

export type ReplaySession = {
	path: string;
	name: string;
};

export type ReplayRequest = {
	/** stream time in milliseconds; unset starts just before the session */
	from?: number;
	speed: number;
	paused: boolean;
};

type ReplayStore = {
	session: ReplaySession | null;
	request: ReplayRequest;

	start: (session: ReplaySession) => void;
	stop: () => void;
	/** changing any of these restarts the replay at the given position */
	control: (request: Partial<ReplayRequest>) => void;
};

export const useReplayStore = create<ReplayStore>((set) => ({
	session: null,
	request: { speed: 1, paused: false },

	start: (session) => set({ session, request: { speed: 1, paused: false } }),
	stop: () => set({ session: null, request: { speed: 1, paused: false } }),
	control: (request) => set((state) => ({ request: { ...state.request, ...request } })),
}));
