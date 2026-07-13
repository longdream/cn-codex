import { create } from "zustand";
import type { UpdateCheckResult } from "../api/update";

interface UpdateState {
  info: UpdateCheckResult | null;
  showModal: boolean;
  checked: boolean;
  setInfo: (info: UpdateCheckResult | null) => void;
  setShowModal: (show: boolean) => void;
  setChecked: (checked: boolean) => void;
  dismiss: () => void;
}

export const useUpdateStore = create<UpdateState>((set) => ({
  info: null,
  showModal: false,
  checked: false,
  setInfo: (info) => set({ info }),
  setShowModal: (showModal) => set({ showModal }),
  setChecked: (checked) => set({ checked }),
  dismiss: () => set({ showModal: false }),
}));
