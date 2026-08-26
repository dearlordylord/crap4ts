type Props<T> = { value: T };

const Component = <T,>(props: Props<T>) => (
    <button onClick={() => props.value}>{props.value}</button>
);

function declaration(value: boolean) {
    return value ? value && value : false;
}

const expression = function (value: number) {
    return value;
};

const object = {
    method() {
        if (true) return 1;
    },
    get getter() {
        return 1;
    },
    set setter(value: number) {
        this.value = value;
    },
    expressionProperty: function () {
        return 1;
    },
    arrowProperty: () => 1,
};

class Example {
    constructor() {
        return;
    }

    static method() {
        return 1;
    }

    get value() {
        return 1;
    }

    set value(next: number) {
        this.next = next;
    }

    field = () => 1;
}

function decisions(value: any) {
    if (value) return;
    for (let index = 0; index < 1; index += 1) {}
    for (const key in value) {}
    for (const item of value) {}
    while (value) break;
    do {
        break;
    } while (value);
    try {
        return value;
    } catch (error) {
        return error;
    }
    switch (value) {
        case 1:
            break;
        case 2:
            break;
        default:
            break;
    }
    const conditional = value ? value : 0;
    value &&= value;
    value ||= value;
    value ??= value;
    return value && value || (value ?? value) || value?.fallback;
}

async function asynchronous() {
    return 1;
}

function* generator() {
    yield 1;
}

function overloaded(value: string): string;
function overloaded(value: number): number;
function overloaded(value: unknown): unknown {
    return value;
}

declare function ambient(): void;

interface Interface {
    run(): void;
}

abstract class Abstract {
    abstract run(): void;
}
