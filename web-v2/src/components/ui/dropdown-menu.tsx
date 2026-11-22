import { ContextMenu as ContextMenuPrimitive, DropdownMenu as DropdownMenuPrimitive } from 'radix-ui'
import { Check } from 'lucide-react'
import type * as React from 'react'
import { cn } from '@/lib/utils'

const content =
  'z-[70] min-w-[184px] origin-(--radix-dropdown-menu-content-transform-origin) rounded-xl border border-line2 bg-bg2 p-1 shadow-lift outline-none data-[state=open]:animate-[pop-in_150ms_var(--ease-out)] data-[state=closed]:animate-[pop-out_100ms_ease-out]'
const item =
  'relative flex h-8 cursor-pointer items-center gap-2.5 rounded-lg px-2.5 text-[13px] text-ink2 outline-none select-none data-[disabled]:pointer-events-none data-[disabled]:opacity-40 data-[highlighted]:bg-bg4 data-[highlighted]:text-ink max-md:h-10 [&_svg]:size-3.5 [&_svg]:shrink-0 [&_svg]:text-ink3 data-[highlighted]:[&_svg]:text-ink2'
const danger = 'data-[highlighted]:bg-bad-soft data-[highlighted]:text-bad data-[highlighted]:[&_svg]:text-bad'

const DropdownMenu = DropdownMenuPrimitive.Root
const DropdownMenuTrigger = DropdownMenuPrimitive.Trigger
const DropdownMenuRadioGroup = DropdownMenuPrimitive.RadioGroup
function DropdownMenuContent({ className, align = 'start', sideOffset = 6, ...props }: React.ComponentProps<typeof DropdownMenuPrimitive.Content>) {
  return (
    <DropdownMenuPrimitive.Portal>
      <DropdownMenuPrimitive.Content align={align} sideOffset={sideOffset} className={cn(content, className)} {...props} />
    </DropdownMenuPrimitive.Portal>
  )
}
function DropdownMenuItem({ className, destructive, ...props }: React.ComponentProps<typeof DropdownMenuPrimitive.Item> & { destructive?: boolean }) {
  return <DropdownMenuPrimitive.Item className={cn(item, destructive && danger, className)} {...props} />
}
function DropdownMenuRadioItem({ className, children, ...props }: React.ComponentProps<typeof DropdownMenuPrimitive.RadioItem>) {
  return (
    <DropdownMenuPrimitive.RadioItem className={cn(item, 'pr-8 data-[state=checked]:text-ink', className)} {...props}>
      {children}
      <DropdownMenuPrimitive.ItemIndicator className="absolute right-2.5">
        <Check className="!size-3.5 !text-ink" />
      </DropdownMenuPrimitive.ItemIndicator>
    </DropdownMenuPrimitive.RadioItem>
  )
}
function DropdownMenuSeparator({ className, ...props }: React.ComponentProps<typeof DropdownMenuPrimitive.Separator>) {
  return <DropdownMenuPrimitive.Separator className={cn('mx-1.5 my-1 h-px bg-line', className)} {...props} />
}
function DropdownMenuLabel({ className, ...props }: React.ComponentProps<typeof DropdownMenuPrimitive.Label>) {
  return <DropdownMenuPrimitive.Label className={cn('px-2.5 pt-2 pb-1 font-mono text-[10.5px] tracking-[0.06em] text-ink3 uppercase', className)} {...props} />
}
function MenuShortcut({ className, ...props }: React.ComponentProps<'span'>) {
  return <span className={cn('ml-auto pl-4 font-mono text-[11px] text-ink3', className)} {...props} />
}

const ContextMenu = ContextMenuPrimitive.Root
const ContextMenuTrigger = ContextMenuPrimitive.Trigger
function ContextMenuContent({ className, ...props }: React.ComponentProps<typeof ContextMenuPrimitive.Content>) {
  return (
    <ContextMenuPrimitive.Portal>
      <ContextMenuPrimitive.Content className={cn(content, 'origin-(--radix-context-menu-content-transform-origin)', className)} {...props} />
    </ContextMenuPrimitive.Portal>
  )
}
function ContextMenuItem({ className, destructive, ...props }: React.ComponentProps<typeof ContextMenuPrimitive.Item> & { destructive?: boolean }) {
  return <ContextMenuPrimitive.Item className={cn(item, destructive && danger, className)} {...props} />
}
function ContextMenuSeparator({ className, ...props }: React.ComponentProps<typeof ContextMenuPrimitive.Separator>) {
  return <ContextMenuPrimitive.Separator className={cn('mx-1.5 my-1 h-px bg-line', className)} {...props} />
}

export {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuLabel,
  MenuShortcut,
  ContextMenu,
  ContextMenuTrigger,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
}
