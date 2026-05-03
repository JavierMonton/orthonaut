I want to build an aplication that will be self-hosted in my home server. 
The application needs to have a frontend, a backend and a database.

The frontend needs to use Headless UI, React and Tailwind CSS so it will be well coded and with nice UI. 

The database can be a sqllite

The backend will be Rust, as defined later. 

The UI will start showing a text box where I can write a link to Wikipedia, and a button to "start". 

The backend needs to run a process that will check ortografy, all the details about this process is defined in requirements.md

The output of the ortografy checker needs to be shown in the UI, as a row per page. So, if a Wikipedia page has been checked, and an ortografy error has been found, a new "row" in the main page will appear with the following information:
- Wikipedia Page Title and link to the official wikipedia page.
- Word that is wrong.
- If multiple words are wrong, it will show one line for each word, everything in the same "row" block. 

As defined in requirements.md, the application will start with a simple way of searching for individual pages, in the future we will use a streaming service, but we don't need to care about it for now. 

When errors have been found in a page, the application will create an entry in the database with the page, which means it will store whe page_id, revision_id and list of wrong words. 

The "row" showing the errors in the main UI will have a "delete" button that will allow a user to delete it from the database. 

When searching a Wikipedia page, the backend will check the ortografy, and if everything is good, it will show a popup or similar saying "everything good", so we know it finished. It'd be nice if it shows a loading spinner or similar while it is searching or checking.

In the input box I will write URLs of the REST service that returns HTML, so the application backend will receive that URL, call the service and pick the HTML. And then, it will do all the things described in requirements.md to check the ortografy. 