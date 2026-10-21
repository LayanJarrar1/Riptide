# Desktop

This project was generated using [Angular CLI](https://github.com/angular/angular-cli) version 20.3.5.

## Development server

To start a local development server, run:

```bash
ng serve
```

Once the server is running, open your browser and navigate to `http://localhost:4200/`. The application will automatically reload whenever you modify any of the source files.

## Code scaffolding

Angular CLI includes powerful code scaffolding tools. To generate a new component, run:

```bash
ng generate component component-name
```

For a complete list of available schematics (such as `components`, `directives`, or `pipes`), run:

```bash
ng generate --help
```

## Building

To build the project run:

```bash
ng build
```

This will compile your project and store the build artifacts in the `dist/` directory. By default, the production build optimizes your application for performance and speed.

## Running unit tests

To execute unit tests with the [Karma](https://karma-runner.github.io) test runner, use the following command:

```bash
ng test
```

## Running end-to-end tests

For end-to-end (e2e) testing, run:

```bash
ng e2e
```

Angular CLI does not come with an end-to-end testing framework by default. You can choose one that suits your needs.

## List performance harness

`e2e/list_perf.py` (Python, Playwright) serves the production build and measures the downloads list
against the mock engine seeded with 10 to 50 000 downloads (`?mockCount=N`, optionally
`&mockActive=K&mockTickMs=T`): DOM rows, heap, time to the first row, scroll frame times, main-thread
time with 100 active downloads updating at 10 Hz, and sort / filter latency. Results are saved to
`e2e/list_perf.md`.

```bash
npm run build
python e2e/list_perf.py --runs 3
python e2e/titlebar_shots.py OUT_DIR   # title bar screenshots (EN / AR, light / dark, menus open)
```

## Additional Resources

For more information on using the Angular CLI, including detailed command references, visit the [Angular CLI Overview and Command Reference](https://angular.dev/tools/cli) page.
