<script lang="ts">
	import { Label } from '@workbench/ui/label';
	import { Switch } from '@workbench/ui/switch';

	let {
		label,
		description,
		checked,
		onCheckedChange
	}: {
		label: string;
		description: string;
		checked: boolean;
		onCheckedChange: (value: boolean) => void;
	} = $props();
</script>

<div class="flex items-center justify-between">
	<div>
		<Label class="text-sm font-medium">{label}</Label>
		<p class="text-xs text-muted-foreground">{description}</p>
	</div>
	<Switch {checked} {onCheckedChange} />
</div>
