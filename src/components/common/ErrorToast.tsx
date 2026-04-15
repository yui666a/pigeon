import { useErrorStore } from "../../stores/errorStore";

export function ErrorToast() {
  const errors = useErrorStore((s) => s.errors);
  const dismissError = useErrorStore((s) => s.dismissError);

  if (errors.length === 0) return null;

  return (
    <div className="fixed bottom-4 right-4 z-50 flex flex-col gap-2">
      {errors.map((error) => (
        <div
          key={error.id}
          className="flex items-start gap-2 rounded-lg bg-red-600 px-4 py-3 text-sm text-white shadow-lg"
        >
          <span className="flex-1">{error.message}</span>
          <button
            onClick={() => dismissError(error.id)}
            className="ml-2 text-red-200 hover:text-white"
          >
            ✕
          </button>
        </div>
      ))}
    </div>
  );
}
