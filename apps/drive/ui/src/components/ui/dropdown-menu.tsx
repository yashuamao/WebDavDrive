import { Menu } from "@base-ui/react/menu";
import { MoreHorizontal } from "lucide-react";
import { Fragment } from "react";

interface MenuAction {
  label: string;
  onSelect: () => void;
  danger?: boolean;
  disabled?: boolean;
  separatorBefore?: boolean;
}

export function DropdownMenu({ label, actions }: { label: string; actions: MenuAction[] }) {
  return (
    <Menu.Root>
      <Menu.Trigger
        className="icon-control"
        aria-label={label}
        onClick={(event) => event.stopPropagation()}
      >
        <MoreHorizontal aria-hidden="true" size={17} />
      </Menu.Trigger>
      <Menu.Portal>
        <Menu.Positioner className="menu-positioner" sideOffset={4} align="end">
          <Menu.Popup className="menu-popup">
            {actions.map((action) => (
              <Fragment key={action.label}>
                {action.separatorBefore ? <Menu.Separator className="menu-separator" /> : null}
                <Menu.Item
                  className="menu-item"
                  data-danger={action.danger || undefined}
                  disabled={action.disabled}
                  onClick={(event) => {
                    event.stopPropagation();
                    action.onSelect();
                  }}
                >
                  {action.label}
                </Menu.Item>
              </Fragment>
            ))}
          </Menu.Popup>
        </Menu.Positioner>
      </Menu.Portal>
    </Menu.Root>
  );
}
