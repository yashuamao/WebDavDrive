import { Select as SelectPrimitive } from "@base-ui/react/select";
import { Check, ChevronDown } from "lucide-react";
import { cn } from "@/lib/utils";

export interface SelectOption {
  value: string;
  label: string;
}

interface SelectProps {
  value: string;
  options: SelectOption[];
  label: string;
  disabled?: boolean;
  className?: string;
  onValueChange: (value: string) => void;
}

export function Select({ value, options, label, disabled, className, onValueChange }: SelectProps) {
  return (
    <SelectPrimitive.Root
      value={value}
      items={options}
      disabled={disabled}
      onValueChange={(next) => onValueChange(String(next))}
    >
      <SelectPrimitive.Trigger className={cn("select-trigger", className)} aria-label={label}>
        <SelectPrimitive.Value />
        <SelectPrimitive.Icon>
          <ChevronDown aria-hidden="true" size={14} />
        </SelectPrimitive.Icon>
      </SelectPrimitive.Trigger>
      <SelectPrimitive.Portal>
        <SelectPrimitive.Positioner className="select-positioner" sideOffset={4} alignItemWithTrigger={false}>
          <SelectPrimitive.Popup className="select-popup">
            <SelectPrimitive.List>
              {options.map((option) => (
                <SelectPrimitive.Item key={option.value} value={option.value} className="select-item">
                  <SelectPrimitive.ItemIndicator className="select-check">
                    <Check aria-hidden="true" size={13} />
                  </SelectPrimitive.ItemIndicator>
                  <SelectPrimitive.ItemText>{option.label}</SelectPrimitive.ItemText>
                </SelectPrimitive.Item>
              ))}
            </SelectPrimitive.List>
          </SelectPrimitive.Popup>
        </SelectPrimitive.Positioner>
      </SelectPrimitive.Portal>
    </SelectPrimitive.Root>
  );
}
