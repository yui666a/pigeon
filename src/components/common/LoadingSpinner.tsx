interface LoadingSpinnerProps {
  message?: string;
}

export function LoadingSpinner({ message }: LoadingSpinnerProps) {
  return (
    <div className="flex flex-col items-center gap-2 py-4">
      <div
        className="h-6 w-6 animate-spin rounded-full border-2 border-blue-600 border-t-transparent"
        role="status"
      />
      {message && <p className="text-sm text-gray-600">{message}</p>}
    </div>
  );
}
