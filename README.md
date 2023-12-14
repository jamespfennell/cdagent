# Continuous deployment agent

This is a WIP agent that runs on my VMs and implements continuous deployment for my lower risk projects.

The agent watches a number of GitHub repositories.
Whenever there is a new successful GitHub actions run on mainline, 
    the agent executes a specified command on the VM. 
All of my GitHub actions build Docker images and push them to Docker hub;
    the command executed by the agent pulls the latest image and recreates the container.
But for simplicity the agent is designed to be "Docker agnostic"
    and can run any command when a new successful build on mainline completes.

The repositories to watch and the VM commands to execute are specified using a config file.
An example of this config file is `example-config.yaml` and the full spec with documentation
    is at `src/config.rs`.

To run the agent in the repository root, simply run `cargo run -- $PATH_TO_CONFIG_FILE`.

## Deploying the agent

As with all my projects, the agent is deployed using Docker.
This introduces some challenges as the vanilla Dockerized deployment of the agent is, of course, sandboxed by default.
The agent generally needs access to many system resources in order to redeploy the configured projects
E.g., it needs to use system commands like `docker-compose`
    and it needs read various `compose.yaml` files on the filesystem.
The solution is to use appropriate Docker file system mounts.

There is of course the question of whether the agent can be used to redeploy itself and the answer
    is unfortunately no.

## License

MIT
