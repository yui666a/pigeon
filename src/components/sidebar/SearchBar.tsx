import { useState, useRef, forwardRef, useImperativeHandle } from "react";

interface SearchBarProps {
  onSearch: (query: string) => void;
  onClear: () => void;
}

export interface SearchBarHandle {
  focus: () => void;
}

export const SearchBar = forwardRef<SearchBarHandle, SearchBarProps>(
  function SearchBar({ onSearch, onClear }, ref) {
    const [value, setValue] = useState("");
    const inputRef = useRef<HTMLInputElement>(null);

    useImperativeHandle(ref, () => ({
      focus: () => inputRef.current?.focus(),
    }));

    const handleKeyDown = (e: React.KeyboardEvent) => {
      if (e.key === "Enter" && value.trim()) {
        onSearch(value.trim());
      } else if (e.key === "Escape") {
        setValue("");
        onClear();
        inputRef.current?.blur();
      }
    };

    return (
      <div className="px-3 py-2">
        <input
          ref={inputRef}
          type="text"
          placeholder="検索..."
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={handleKeyDown}
          className="w-full rounded border border-gray-300 bg-white px-3 py-1.5 text-sm focus:border-blue-400 focus:outline-none"
        />
      </div>
    );
  }
);
