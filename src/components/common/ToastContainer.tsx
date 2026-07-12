import { useErrorStore, type ToastKind } from "../../stores/errorStore";

const KIND_STYLES: Record<ToastKind, { toast: string; dismiss: string }> = {
  error: { toast: "bg-red-600", dismiss: "text-red-200 hover:text-white" },
  success: {
    toast: "bg-green-600",
    dismiss: "text-green-200 hover:text-white",
  },
};

export function ToastContainer() {
  const toasts = useErrorStore((s) => s.toasts);
  const dismissToast = useErrorStore((s) => s.dismissToast);

  if (toasts.length === 0) return null;

  return (
    <div className="fixed bottom-4 right-4 z-50 flex flex-col gap-2">
      {toasts.map((toast) => {
        const styles = KIND_STYLES[toast.kind];
        return (
          <div
            key={toast.id}
            className={`flex items-start gap-2 rounded-lg px-4 py-3 text-sm text-white shadow-lg ${styles.toast}`}
          >
            <span className="flex-1">{toast.message}</span>
            <button
              onClick={() => dismissToast(toast.id)}
              className={`ml-2 ${styles.dismiss}`}
            >
              ✕
            </button>
          </div>
        );
      })}
    </div>
  );
}
