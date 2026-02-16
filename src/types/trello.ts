export interface TrelloCredentials {
	apiKey: string;
	token: string;
}

export interface TrelloBoard {
	id: string;
	name: string;
	url: string;
}

export interface TrelloList {
	id: string;
	name: string;
	pos: number;
}

export interface TrelloCard {
	id: string;
	name: string;
	desc: string;
	idList: string;
	url: string;
	labels: TrelloLabel[];
	pos: number;
	due: string | null;
}

export interface TrelloLabel {
	id: string;
	name: string;
	color: string | null;
}

export interface TrelloBoardData {
	board: TrelloBoard;
	columns: TrelloColumnData[];
}

export interface TrelloColumnData {
	column: TrelloList;
	cards: TrelloCard[];
}

export interface BoardConfig {
	boardId: string;
	boardName: string;
	hiddenColumns: string[];
	linkAction?: MergeAction;
	mergeAction?: MergeAction;
}

export interface MergeAction {
	moveToColumnId?: string;
	moveToColumnName?: string;
	addLabelIds: string[];
	removeLabelIds: string[];
}

export interface TaskLink {
	cardId: string;
	boardId: string;
	branch: string;
	worktreePath?: string;
	projectPath: string;
}

export interface TrelloProjectConfig {
	boards: BoardConfig[];
	taskLinks: TaskLink[];
}
